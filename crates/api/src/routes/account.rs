//! `/public/account`: self-service account deletion and personal-data
//! export (UK legal audit LEG-4, 2026-09-27; UK GDPR Arts. 15, 17 and 20).
//! The data side lives in `data::account`.
//!
//! * `GET /public/account/export` -- every row of personal data held about
//!   the caller, as a downloadable JSON file.
//! * `DELETE /public/account` -- deletes the caller's account and all their
//!   personal data. Requires a JSON body
//!   `{"confirm": "delete my account"}` and a same-origin `Origin`/`Referer`
//!   (strict, like `logout`: both absent is refused too), on top of the
//!   router-wide L4 `auth::reject_cross_origin_cookie_mutation` layer.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::app::{App, Router};
use crate::auth::{self, AuthenticatedUser};
use crate::data::account;

pub fn router() -> Router {
    Router::new()
        .route("/account", axum::routing::delete(delete_account))
        .route("/account/export", axum::routing::get(export_account))
}

#[derive(Debug, Deserialize)]
struct DeleteAccountRequest {
    confirm: String,
}

/// Whether `confirm` is the required phrase, ignoring case and surrounding
/// whitespace.
fn is_confirmed(confirm: &str) -> bool {
    confirm
        .trim()
        .eq_ignore_ascii_case(account::DELETE_ACCOUNT_CONFIRMATION)
}

/// The strict same-origin check `logout` uses: a real browser `DELETE`
/// always carries `Origin`, so refusing when neither `Origin` nor `Referer`
/// is present costs a legitimate request nothing. `None` expected origin
/// (unparseable config) disables the check, as for `logout`.
fn is_same_origin_request(headers: &HeaderMap, expected_origin: Option<&str>) -> bool {
    match expected_origin {
        None => true,
        Some(expected_origin) => auth::is_same_origin(
            headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()),
            headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
            expected_origin,
        ),
    }
}

async fn delete_account(
    State(app): State<App>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Json(body): Json<DeleteAccountRequest>,
) -> Response {
    let expected_origin = auth::expected_browser_origin(&app.config.sso_redirect_url);
    if !is_same_origin_request(&headers, expected_origin.as_deref()) {
        tracing::warn!(
            "account deletion rejected: Origin/Referer did not match this app's own origin"
        );
        return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
    }
    if !is_confirmed(&body.confirm) {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "to delete your account, send {{\"confirm\": \"{}\"}}",
                account::DELETE_ACCOUNT_CONFIRMATION
            ),
        )
            .into_response();
    }

    match account::delete_account(&app.database, &user.id).await {
        Ok(outcome) => {
            // No user id in the log line: the account is gone, and the log
            // should not keep a record of who it was.
            tracing::info!(?outcome, "account deleted at the user's request");
        }
        Err(err) => {
            tracing::error!(error = ?err, "account deletion failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "account deletion failed").into_response();
        }
    }

    // Every session went with the account; clear this browser's cookie too.
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(
            auth::SESSION_COOKIE_NAME,
            app.config.sso_redirect_url.starts_with("https://"),
        ))
        .expect("cookie header value is always valid ASCII"),
    );
    response
}

async fn export_account(State(app): State<App>, user: AuthenticatedUser) -> Response {
    let export = match account::export_account(&app.database, &user.id).await {
        Ok(Some(export)) => export,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such account").into_response(),
        Err(err) => {
            tracing::error!(error = ?err, "account export failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "account export failed").into_response();
        }
    };
    let filename = format!(
        "distant-signal-my-data-{}.json",
        export.exported_at.format("%Y-%m-%d")
    );
    let mut response = Json(export).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .expect("filename is ASCII"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_must_be_the_exact_phrase_ignoring_case_and_whitespace() {
        assert!(is_confirmed("delete my account"));
        assert!(is_confirmed("  Delete My Account \n"));
        assert!(!is_confirmed(""));
        assert!(!is_confirmed("delete"));
        assert!(!is_confirmed("yes"));
        assert!(!is_confirmed("delete my account please"));
    }

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(name.clone(), HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn same_origin_check_is_strict() {
        let expected = Some("https://rail.example.com");
        assert!(is_same_origin_request(
            &headers(&[(header::ORIGIN, "https://rail.example.com")]),
            expected
        ));
        assert!(is_same_origin_request(
            &headers(&[(header::REFERER, "https://rail.example.com/account")]),
            expected
        ));
        assert!(!is_same_origin_request(
            &headers(&[(header::ORIGIN, "https://evil.example.com")]),
            expected
        ));
        assert!(
            !is_same_origin_request(&HeaderMap::new(), expected),
            "no Origin and no Referer is refused"
        );
        assert!(is_same_origin_request(&HeaderMap::new(), None));
    }
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::users::insert_session;

    const ORIGIN: &str = "https://example.invalid";

    /// Copy of `routes::groups::db_tests::test_app`: every
    /// `ServiceArguments` field an inert placeholder. `sso_redirect_url`
    /// (`https://example.invalid/callback`) makes [`ORIGIN`] this app's
    /// own origin for the same-origin check.
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            migration_database_url: None,
            redis_url: "redis://127.0.0.1:0".to_string(),
            redis_password: None,
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
            internal_oauth_group_corpus: "svc-corpus-ingest".to_string(),
            internal_oauth_group_mcp: "srv-ds-mcp".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            chatbot_access: crate::data::config::ChatbotAccessMode::Group,
            admin_group: String::new(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
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
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // see `AppState::redis`'s doc comment. No route in this file
            // touches Redis at all.
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    /// This file's own `router()`, mounted unprefixed exactly as `main.rs`
    /// mounts it inside `public_router()`, turned into a `tower::Service` a
    /// test can drive with `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
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

    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $1) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed user");
        let raw_token = format!("test-raw-session-token-for-{user_id}");
        insert_session(pool, &hash_session_token(&raw_token), user_id, 14)
            .await
            .expect("seed session");
        raw_token
    }

    fn delete_request(token: Option<&str>, origin: Option<&str>, body: &str) -> Request<Body> {
        let mut builder = Request::builder()
            .method("DELETE")
            .uri("/account")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn user_exists(pool: &PgPool, user_id: &str) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .unwrap()
            > 0
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
    async fn delete_account_route_requires_session_same_origin_and_confirmation() {
        let pool = connect().await;
        let user = "acct-route-user";
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let token = seed_session(&pool, user).await;
        let router = test_router(test_app(pool.clone()));
        let good = r#"{"confirm": "delete my account"}"#;

        let res = router
            .clone()
            .oneshot(delete_request(None, Some(ORIGIN), good))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let res = router
            .clone()
            .oneshot(delete_request(
                Some(&token),
                Some("https://evil.example.com"),
                good,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let res = router
            .clone()
            .oneshot(delete_request(Some(&token), None, good))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::FORBIDDEN,
            "no Origin/Referer is refused"
        );

        let res = router
            .clone()
            .oneshot(delete_request(
                Some(&token),
                Some(ORIGIN),
                r#"{"confirm": "yes"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let res = router
            .clone()
            .oneshot(delete_request(Some(&token), Some(ORIGIN), ""))
            .await
            .unwrap();
        assert!(res.status().is_client_error(), "a missing body is refused");
        assert!(
            user_exists(&pool, user).await,
            "nothing deleted by a refused request"
        );

        let res = router
            .clone()
            .oneshot(delete_request(Some(&token), Some(ORIGIN), good))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.starts_with("distant_signal_session="), "{cookie}");
        assert!(cookie.contains("Max-Age=0"), "{cookie}");
        assert!(!user_exists(&pool, user).await);

        // The session died with the account.
        let res = router
            .oneshot(delete_request(Some(&token), Some(ORIGIN), good))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
    async fn export_route_returns_an_attachment_for_the_caller_only() {
        let pool = connect().await;
        let user = "acct-route-export";
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        let token = seed_session(&pool, user).await;
        let router = test_router(test_app(pool.clone()));

        let anonymous = Request::builder()
            .uri("/account/export")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(anonymous).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );

        let req = Request::builder()
            .uri("/account/export")
            .header(header::COOKIE, format!("distant_signal_session={token}"))
            .body(Body::empty())
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let disposition = res
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            disposition.starts_with("attachment; filename=\"distant-signal-my-data-"),
            "{disposition}"
        );
        assert_eq!(
            res.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["account"]["id"], user);
        assert_eq!(json["sessions"].as_array().unwrap().len(), 1);

        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
    }
}
