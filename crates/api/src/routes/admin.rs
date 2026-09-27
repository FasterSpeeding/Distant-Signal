//! `/public/admin/...`: operator actions gated on the `ADMIN_GROUP`
//! Authentik group (M14, 2026-09-27). Today there is exactly one: ending all
//! of another user's sessions.
//!
//! "Admin" is membership of `ServiceArguments::admin_group` in the caller's
//! `users.groups`, the same `groups` OIDC claim `chatbot_access_group` gates
//! on. Empty (the default) turns the feature off: nobody is an admin.
//! Membership is read from `users.groups`, which is written only at login, so
//! a change in Authentik takes effect at that admin's next login (or at once
//! if another admin revokes their sessions). See docs/session-revocation.md.
//!
//! There is no admin UI; call it from a logged-in browser session, e.g. the
//! devtools console on the site:
//! `fetch('/api/admin/users/revoke-sessions', {method: 'POST', headers:
//! {'Content-Type': 'application/json'}, body: JSON.stringify({email:
//! 'someone@example.com'})})`.

use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{self, AuthenticatedUser};
use crate::data::users;

pub fn router() -> Router {
    Router::new().route(
        "/admin/users/revoke-sessions",
        axum::routing::post(revoke_user_sessions),
    )
}

/// Is a user with `groups` an admin under `admin_group`? Never when
/// `admin_group` is empty or blank: an unset group must not match a user
/// who (somehow) has an empty-string group.
pub(crate) fn is_admin(groups: &[String], admin_group: &str) -> bool {
    let admin_group = admin_group.trim();
    !admin_group.is_empty() && groups.iter().any(|group| group == admin_group)
}

/// Exactly one of the two names the target user.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevokeRequest {
    /// The OIDC subject (`users.id`).
    user_id: Option<String>,
    /// The user's verified email, matched case-insensitively. Refused as
    /// ambiguous if more than one user has it.
    email: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RevokeResponse {
    user_id: String,
    sessions_revoked: u64,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({ "error": code }))).into_response()
}

/// `POST /public/admin/users/revoke-sessions`: ends every session the target
/// user holds (`users::revoke_all_sessions`: `sessions_invalidated_at` marker
/// plus delete, in one transaction). Their next request is anonymous; they
/// can log in again at once, and that login re-reads their groups from
/// Authentik -- which is how an admin makes a group removal take effect
/// before the 14-day session TTL. To keep someone out, disable them in
/// Authentik as well.
///
/// Guards, in order: a live session (`401`); the strict Origin/Referer check
/// `logout` and `revoke-others` use (`403`), on top of the router-wide L4
/// guard; admin-group membership (`403 not_admin`). Every revocation, and
/// every refused attempt by a logged-in non-admin, is logged with
/// `audit = true`.
async fn revoke_user_sessions(
    State(app): State<App>,
    caller: AuthenticatedUser,
    headers: axum::http::HeaderMap,
    body: Result<Json<RevokeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let expected_origin = auth::expected_browser_origin(&app.config.sso_redirect_url);
    if let Some(expected_origin) = expected_origin.as_deref()
        && !auth::is_same_origin(
            headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()),
            headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
            expected_origin,
        )
    {
        tracing::warn!("admin revoke rejected: Origin/Referer did not match this app's own origin");
        return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
    }

    if !is_admin(&caller.groups, &app.config.admin_group) {
        tracing::warn!(
            audit = true,
            event = "admin_session_revoke_denied",
            caller_user_id = %caller.id,
            "non-admin attempted to revoke another user's sessions"
        );
        return error(StatusCode::FORBIDDEN, "not_admin");
    }

    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let non_blank = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let target = match (non_blank(request.user_id), non_blank(request.email)) {
        (Some(user_id), None) => user_id,
        (None, Some(email)) => match users::find_user_ids_by_email(&app.database, &email).await {
            Ok(ids) => match ids.as_slice() {
                [] => return error(StatusCode::NOT_FOUND, "user_not_found"),
                [only] => only.clone(),
                _ => return error(StatusCode::CONFLICT, "email_ambiguous"),
            },
            Err(err) => {
                tracing::error!(error = ?err, "admin revoke: user lookup by email failed");
                return error(StatusCode::INTERNAL_SERVER_ERROR, "lookup_failed");
            }
        },
        _ => {
            return error(
                StatusCode::BAD_REQUEST,
                "give_exactly_one_of_userId_or_email",
            );
        }
    };

    match users::revoke_all_sessions(&app.database, &target).await {
        Ok(Some(sessions_revoked)) => {
            tracing::warn!(
                audit = true,
                event = "admin_session_revoke",
                admin_user_id = %caller.id,
                target_user_id = %target,
                sessions_revoked,
                "admin revoked all of a user's sessions"
            );
            Json(RevokeResponse {
                user_id: target,
                sessions_revoked,
            })
            .into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "user_not_found"),
        Err(err) => {
            tracing::error!(error = ?err, "admin revoke: failed to revoke sessions");
            error(StatusCode::INTERNAL_SERVER_ERROR, "revoke_failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_admin;

    #[test]
    fn membership_of_the_configured_group_is_admin() {
        let groups = vec!["mcp-users".to_string(), "ds-admins".to_string()];
        assert!(is_admin(&groups, "ds-admins"));
        assert!(!is_admin(&groups, "other-admins"));
    }

    #[test]
    fn an_empty_admin_group_means_nobody_is_admin() {
        let groups = vec![String::new(), " ".to_string()];
        assert!(!is_admin(&groups, ""));
        assert!(!is_admin(&groups, "  "));
    }
}

/// DB-gated, end to end through the real routers: the admin endpoint and
/// the back-channel logout endpoint (`routes::auth`), against a mocked
/// Authentik JWKS for the latter. Same oneshot pattern as
/// `routes::chatbot::db_tests`.
#[cfg(test)]
mod session_revocation_db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::backchannel_logout::test_support::{CLIENT_ID, logout_claims};
    use crate::auth::hash_session_token;
    use crate::auth::internal_oauth::test_support::{mock_authentik, sign_token_with_typ};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::users::{get_session_with_user, insert_session};

    const ADMIN_GROUP: &str = "distant-signal-admins";
    const ORIGIN: &str = "https://example.invalid";

    fn test_app(pool: PgPool, issuer: &str, admin_group: &str) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            admin_group: admin_group.to_string(),
            sso_issuer_url: issuer.to_string(),
            sso_client_id: CLIENT_ID.to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: format!("{ORIGIN}/callback"),
            sso_post_login_redirect_url: format!("{ORIGIN}/"),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            metrics_port: 9091,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
            past_travel_retention_days: 548,
            stale_push_subscription_days: 365,
            inactive_account_retention_days: 0,
        };

        std::sync::Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: issuer.to_string(),
                client_id: CLIENT_ID.to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: format!("{ORIGIN}/callback"),
            })
            .expect("construct oidc client"),
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .merge(crate::routes::auth::router())
            .with_state(app)
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Seeds a user with `groups` and one live session per entry of
    /// `tokens`, returning nothing; the raw tokens are the cookie values.
    async fn seed_user(pool: &PgPool, user_id: &str, groups: &[&str], tokens: &[&str]) {
        let groups: Vec<String> = groups.iter().map(|g| g.to_string()).collect();
        sqlx::query(
            "INSERT INTO users (id, email, name, groups) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO UPDATE SET groups = EXCLUDED.groups, email = EXCLUDED.email, \
             sessions_invalidated_at = NULL",
        )
        .bind(user_id)
        .bind(format!("{}@example.com", user_id.to_lowercase()))
        .bind(user_id)
        .bind(&groups)
        .execute(pool)
        .await
        .expect("seed fixture user");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("clear fixture sessions");
        for token in tokens {
            insert_session(pool, &hash_session_token(token), user_id, 14)
                .await
                .expect("seed fixture session");
        }
    }

    async fn cleanup(pool: &PgPool, user_ids: &[&str]) {
        for id in user_ids {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .expect("cleanup fixture user");
        }
    }

    async fn session_alive(pool: &PgPool, token: &str) -> bool {
        get_session_with_user(pool, &hash_session_token(token))
            .await
            .expect("session lookup")
            .is_some()
    }

    async fn send(router: axum::Router, req: Request<Body>) -> (StatusCode, Value) {
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    fn admin_request(caller_token: &str, origin: Option<&str>, body: Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/admin/users/revoke-sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::COOKIE,
                format!("distant_signal_session={caller_token}"),
            );
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        builder
            .body(Body::from(body.to_string()))
            .expect("build request")
    }

    fn backchannel_request(token: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/auth/backchannel-logout")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("logout_token={token}")))
            .expect("build request")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api session_revocation_db_tests \
                -- --ignored --test-threads=1`"]
    async fn an_admin_revokes_every_session_of_the_target_user() {
        let pool = connect().await;
        seed_user(&pool, "TEST-ADMIN-REVOKER", &[ADMIN_GROUP], &["admin-tok"]).await;
        seed_user(&pool, "TEST-ADMIN-TARGET", &[], &["target-a", "target-b"]).await;
        let router = test_router(test_app(
            pool.clone(),
            "https://example.invalid",
            ADMIN_GROUP,
        ));

        let (status, body) = send(
            router.clone(),
            admin_request(
                "admin-tok",
                Some(ORIGIN),
                json!({"userId": "TEST-ADMIN-TARGET"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["userId"], "TEST-ADMIN-TARGET");
        assert_eq!(body["sessionsRevoked"], 2);
        assert!(!session_alive(&pool, "target-a").await);
        assert!(!session_alive(&pool, "target-b").await);
        assert!(
            session_alive(&pool, "admin-tok").await,
            "the admin's own session is untouched"
        );

        // By email, case-insensitively, after the target logs in again.
        seed_user(&pool, "TEST-ADMIN-TARGET", &[], &["target-c"]).await;
        let (status, body) = send(
            router.clone(),
            admin_request(
                "admin-tok",
                Some(ORIGIN),
                json!({"email": "TEST-admin-target@EXAMPLE.com"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(!session_alive(&pool, "target-c").await);

        let (status, body) = send(
            router,
            admin_request(
                "admin-tok",
                Some(ORIGIN),
                json!({"userId": "TEST-NO-SUCH-USER"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

        cleanup(&pool, &["TEST-ADMIN-REVOKER", "TEST-ADMIN-TARGET"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api session_revocation_db_tests \
                -- --ignored --test-threads=1`"]
    async fn a_non_admin_a_cross_site_request_and_an_unset_admin_group_are_refused() {
        let pool = connect().await;
        seed_user(
            &pool,
            "TEST-ADMIN-NONADMIN",
            &["mcp-users"],
            &["nonadmin-tok"],
        )
        .await;
        seed_user(
            &pool,
            "TEST-ADMIN-REVOKER2",
            &[ADMIN_GROUP],
            &["admin2-tok"],
        )
        .await;
        seed_user(&pool, "TEST-ADMIN-TARGET2", &[], &["target2-a"]).await;
        let body = json!({"userId": "TEST-ADMIN-TARGET2"});

        let router = test_router(test_app(
            pool.clone(),
            "https://example.invalid",
            ADMIN_GROUP,
        ));
        let (status, resp) = send(
            router.clone(),
            admin_request("nonadmin-tok", Some(ORIGIN), body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(resp["error"], "not_admin");

        // An admin, but cross-site, or with no Origin/Referer at all.
        for origin in [Some("https://evil.invalid"), None] {
            let (status, _) = send(
                router.clone(),
                admin_request("admin2-tok", origin, body.clone()),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "origin {origin:?}");
        }

        // No session at all.
        let (status, _) = send(
            router,
            Request::builder()
                .method("POST")
                .uri("/admin/users/revoke-sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, ORIGIN)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Feature off (the default): even a member of the would-be group is
        // refused.
        let router = test_router(test_app(pool.clone(), "https://example.invalid", ""));
        let (status, _) = send(router, admin_request("admin2-tok", Some(ORIGIN), body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        assert!(
            session_alive(&pool, "target2-a").await,
            "no refused request may revoke anything"
        );
        cleanup(
            &pool,
            &[
                "TEST-ADMIN-NONADMIN",
                "TEST-ADMIN-REVOKER2",
                "TEST-ADMIN-TARGET2",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api session_revocation_db_tests \
                -- --ignored --test-threads=1`"]
    async fn a_valid_backchannel_logout_token_revokes_the_users_sessions() {
        let pool = connect().await;
        let (server, _) = mock_authentik().await;
        let issuer = server.uri();
        seed_user(&pool, "TEST-BCL-USER", &[], &["bcl-a", "bcl-b"]).await;
        seed_user(&pool, "TEST-BCL-BYSTANDER", &[], &["bcl-other"]).await;
        let router = test_router(test_app(pool.clone(), &issuer, ""));

        let token = sign_token_with_typ(
            &logout_claims(&issuer, "TEST-BCL-USER", |_| {}),
            "logout+jwt",
        );
        let response = router
            .clone()
            .oneshot(backchannel_request(&token))
            .await
            .expect("oneshot");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(!session_alive(&pool, "bcl-a").await);
        assert!(!session_alive(&pool, "bcl-b").await);
        assert!(
            session_alive(&pool, "bcl-other").await,
            "other users are untouched"
        );

        // A valid token for a user this app has never seen is still 200.
        let token = sign_token_with_typ(
            &logout_claims(&issuer, "TEST-BCL-NEVER-SEEN", |_| {}),
            "logout+jwt",
        );
        let (status, _) = send(router, backchannel_request(&token)).await;
        assert_eq!(status, StatusCode::OK);

        cleanup(&pool, &["TEST-BCL-USER", "TEST-BCL-BYSTANDER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api session_revocation_db_tests \
                -- --ignored --test-threads=1`"]
    async fn invalid_backchannel_logout_tokens_are_rejected_and_revoke_nothing() {
        let pool = connect().await;
        let (server, _) = mock_authentik().await;
        let issuer = server.uri();
        seed_user(&pool, "TEST-BCL-KEEP", &[], &["bcl-keep"]).await;
        let router = test_router(test_app(pool.clone(), &issuer, ""));
        let now = chrono::Utc::now().timestamp();
        let sub = "TEST-BCL-KEEP";

        let valid = sign_token_with_typ(&logout_claims(&issuer, sub, |_| {}), "logout+jwt");
        let mut parts: Vec<String> = valid.split('.').map(str::to_string).collect();
        parts[2] = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            b"forged-signature-bytes",
        );
        let bad_signature = parts.join(".");

        let cases: Vec<(&str, String)> = vec![
            ("bad signature", bad_signature),
            (
                "wrong aud",
                sign_token_with_typ(
                    &logout_claims(&issuer, sub, |c| c["aud"] = json!("some-other-client")),
                    "logout+jwt",
                ),
            ),
            (
                "expired",
                sign_token_with_typ(
                    &logout_claims(&issuer, sub, |c| {
                        c["iat"] = json!(now - 7200);
                        c["exp"] = json!(now - 3600);
                    }),
                    "logout+jwt",
                ),
            ),
            (
                "nonce present",
                sign_token_with_typ(
                    &logout_claims(&issuer, sub, |c| c["nonce"] = json!("abc")),
                    "logout+jwt",
                ),
            ),
            (
                "wrong iss",
                sign_token_with_typ(
                    &logout_claims("https://evil.invalid/", sub, |_| {}),
                    "logout+jwt",
                ),
            ),
            (
                "no logout event",
                sign_token_with_typ(
                    &logout_claims(&issuer, sub, |c| {
                        c.as_object_mut().unwrap().remove("events");
                    }),
                    "logout+jwt",
                ),
            ),
            (
                "sid only",
                sign_token_with_typ(
                    &logout_claims(&issuer, sub, |c| {
                        c.as_object_mut().unwrap().remove("sub");
                    }),
                    "logout+jwt",
                ),
            ),
            ("not a jwt", "garbage".to_string()),
        ];
        for (name, token) in cases {
            let (status, body) = send(router.clone(), backchannel_request(&token)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
            assert_eq!(body["error"], "invalid_request", "{name}");
        }

        // No form field at all.
        let (status, _) = send(
            router,
            Request::builder()
                .method("POST")
                .uri("/auth/backchannel-logout")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("something_else=1"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        assert!(
            session_alive(&pool, "bcl-keep").await,
            "no rejected token may revoke anything"
        );
        cleanup(&pool, &["TEST-BCL-KEEP"]).await;
    }
}
