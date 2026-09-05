//! `/public/chatbot/access` -- a beta/feature-flag gate for the `/chat`
//! page's own visibility. NOT spend-protection: that was this table's
//! original purpose (dual-mode design's Decision 5), back when Option B
//! held a DS-funded Anthropic key server-side. Once each user supplies
//! their own Anthropic key directly to their own browser (see
//! docs/superpowers/specs/2026-09-02-embedded-chatbot-option-b-client-side-tokens-design.md's
//! Decision 4), there is no DS spend left to protect -- this is now purely
//! a soft-launch/access-control gate, independent from and not a proxy for
//! `distant-signal-mcp`'s own `mcp-users`/`mcp-live-boards` access groups
//! (which gate the tools themselves, for a materially different
//! population -- Option C's arbitrary Claude.ai users included).
//!
//! One caller: `frontend/app/chat/page.tsx`'s own page-load gate. (The
//! former second caller, `orchestrator/`'s `checkChatbotAccess` -- "the
//! actual cost-protecting check, since a request can reach the
//! orchestrator without ever rendering the page" -- no longer exists;
//! `orchestrator/` was removed entirely, see the client-side-tokens plan's
//! Task 5.)

use axum::Json;
use serde_json::{Value, json};

use crate::app::Router;
use crate::auth::ChatbotAuthorizedUser;

pub fn router() -> Router {
    Router::new().route("/chatbot/access", axum::routing::get(access))
}

/// `401` (via `ChatbotAuthorizedUser`'s inner `AuthenticatedUser`, unchanged)
/// for no session at all; `403 { "error": "chatbot_not_available" }` for a
/// logged-in, non-allowlisted user; `200 { "allowed": true }` otherwise --
/// never `404`, see `ChatbotAuthorizedUser`'s own doc comment.
async fn access(ChatbotAuthorizedUser(_user): ChatbotAuthorizedUser) -> Json<Value> {
    Json(json!({ "allowed": true }))
}

/// HTTP-layer tests for `GET /chatbot/access`'s three outcomes. Follows the
/// `db_tests` convention (`test_app`/`test_router`/`seed_session` built by
/// hand against a real `App`, exercised through the real `axum::Router` via
/// `tower::ServiceExt::oneshot`) established in `crate::routes::lines::db_tests`
/// and repeated in several sibling route modules since -- kept as this
/// file's own colocated copy rather than importing another file's private
/// helpers, per those modules' own "promote only once a third file needs
/// them" doc comments; still not promoted here, that's a separate decision
/// for this plan's controller to make.
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::users::insert_session;

    fn test_app(pool: PgPool) -> App {
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
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
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
        })
    }

    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
    }

    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");

        let raw_token = format!("test-raw-session-token-for-{user_id}");
        insert_session(pool, &hash_session_token(&raw_token), user_id, 14)
            .await
            .expect("seed fixture session");
        raw_token
    }

    async fn allowlist(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "INSERT INTO chatbot_allowed_users (user_id) VALUES ($1) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("insert allowlist row");
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        // chatbot_allowed_users/sessions both cascade via ON DELETE CASCADE.
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn request(router: axum::Router, raw_token: Option<&str>) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri("/chatbot/access");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                chatbot_access -- --ignored --test-threads=1`"]
    async fn anonymous_request_is_401() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let (status, _body) = request(router, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                chatbot_access -- --ignored --test-threads=1`"]
    async fn logged_in_but_not_allowlisted_is_403_not_404() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-CHATBOT-NOT-ALLOWED").await;
        let router = test_router(test_app(pool.clone()));

        let (status, body) = request(router, Some(&token)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("chatbot_not_available")
        );

        cleanup_user(&pool, "TEST-CHATBOT-NOT-ALLOWED").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                chatbot_access -- --ignored --test-threads=1`"]
    async fn allowlisted_user_gets_200_allowed_true() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-CHATBOT-ALLOWED").await;
        allowlist(&pool, "TEST-CHATBOT-ALLOWED").await;
        let router = test_router(test_app(pool.clone()));

        let (status, body) = request(router, Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.get("allowed").and_then(Value::as_bool), Some(true));

        cleanup_user(&pool, "TEST-CHATBOT-ALLOWED").await;
    }
}
