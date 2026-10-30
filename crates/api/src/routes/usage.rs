//! Opt-in usage analytics (DS-190; `crate::usage_events`).
//!
//! - `POST /public/events`: one event from a browser that opted in. Always
//!   `204`, counted or not, so a client learns nothing from the answer
//!   and has nothing to retry. Not counted when the request carries
//!   `Sec-GPC: 1` or `DNT: 1`, when a signed-in caller's account has not
//!   opted in, or when the event is not on the allowlist. Rate-limited per
//!   client IP (`rate_limit::LimitClass::Events`).
//! - `GET`/`PUT /public/preferences/analytics`: a signed-in user's choice,
//!   `{"optIn": bool}`, off by default. A logged-out browser keeps its own
//!   choice and the api trusts it (nothing it sends identifies anyone).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::usage;
use crate::usage_events::{self, DropReason, EventBody};

pub fn router() -> Router {
    Router::new()
        .route(
            "/events",
            axum::routing::post(post_event)
                .layer(DefaultBodyLimit::max(usage_events::MAX_BODY_BYTES)),
        )
        .route(
            "/preferences/analytics",
            axum::routing::get(get_analytics).put(put_analytics),
        )
}

async fn post_event(
    State(app): State<App>,
    headers: HeaderMap,
    user: Result<OptionalAuthenticatedUser, (StatusCode, String)>,
    body: Bytes,
) -> StatusCode {
    if let Err(reason) = count_event(&app, &headers, user, &body).await {
        reason.record();
    }
    StatusCode::NO_CONTENT
}

/// Counts one event, or says why not.
async fn count_event(
    app: &App,
    headers: &HeaderMap,
    user: Result<OptionalAuthenticatedUser, (StatusCode, String)>,
    body: &[u8],
) -> Result<(), DropReason> {
    if usage_events::has_privacy_signal(headers) {
        return Err(DropReason::PrivacySignal);
    }
    let body: EventBody = serde_json::from_slice(body).map_err(|_| DropReason::Invalid)?;
    let event = usage_events::validate(&body).ok_or(DropReason::Invalid)?;
    let signed_in = match user {
        Ok(OptionalAuthenticatedUser(None)) => false,
        Ok(OptionalAuthenticatedUser(Some(user))) if user.analytics_opt_in => true,
        Ok(OptionalAuthenticatedUser(Some(_))) => return Err(DropReason::NotOptedIn),
        // The session lookup failed: whether this caller opted in is
        // unknown, so it is not counted.
        Err(_) => return Err(DropReason::Error),
    };
    usage::increment(
        &app.database,
        super::london_today(),
        event.event,
        &event.dim,
        signed_in,
    )
    .await
    .map_err(|err| {
        tracing::warn!(error = ?err, "couldn't count a usage event");
        DropReason::Error
    })?;
    usage_events::record_usage(&event, signed_in);
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnalyticsPreference {
    opt_in: bool,
}

async fn get_analytics(user: AuthenticatedUser) -> Json<AnalyticsPreference> {
    Json(AnalyticsPreference {
        opt_in: user.analytics_opt_in,
    })
}

async fn put_analytics(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<AnalyticsPreference>,
) -> Result<StatusCode, (StatusCode, String)> {
    let updated = usage::set_opt_in(&app.database, &user.id, body.opt_in)
        .await
        .map_err(|err| {
            if let Some(unavailable) = crate::unavailable::response_for(&err) {
                return unavailable;
            }
            tracing::error!(error = ?err, "couldn't save the analytics choice");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "couldn't save the analytics choice".to_string(),
            )
        })?;
    if !updated {
        return Err((StatusCode::UNAUTHORIZED, "no such user".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    fn app() -> axum::Router {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .unwrap();
        Router::new()
            .nest("/public", router())
            .with_state(crate::test_support::inert_app(pool))
    }

    async fn post(router: &axum::Router, body: &str, headers: &[(&str, &str)]) -> StatusCode {
        let mut request = Request::builder()
            .method("POST")
            .uri("/public/events")
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        router
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
            .status()
    }

    fn dropped(rendered: &str, reason: &str) -> u64 {
        let prefix =
            format!("distant_signal_api_usage_events_dropped_total{{reason=\"{reason}\"}} ");
        rendered
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .map_or(0, |n| n.trim().parse().unwrap())
    }

    /// No database: these never reach it. A privacy signal or an invalid
    /// event is dropped before any lookup, and both still answer 204.
    #[tokio::test]
    async fn privacy_signals_and_invalid_events_are_dropped_with_a_204() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let router = app();
        let valid = r#"{"name":"page_view","route_template":"/lines"}"#;
        assert_eq!(
            post(&router, valid, &[("sec-gpc", "1")]).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            post(&router, valid, &[("dnt", "1")]).await,
            StatusCode::NO_CONTENT
        );
        for invalid in [
            "not json",
            r#"{"name":"page_view","route_template":"/lines/northern"}"#,
            r#"{"name":"page_view","route_template":"/lines","ip":"1.2.3.4"}"#,
            r#"{"name":"login","route_template":"/"}"#,
        ] {
            assert_eq!(post(&router, invalid, &[]).await, StatusCode::NO_CONTENT);
        }
        let rendered = handle.render();
        assert_eq!(dropped(&rendered, "privacy_signal"), 2, "{rendered}");
        assert_eq!(dropped(&rendered, "invalid"), 4, "{rendered}");
    }

    #[tokio::test]
    async fn an_oversized_body_is_refused() {
        let router = app();
        let body = format!(
            r#"{{"name":"page_view","route_template":"/","props":{{"x":"{}"}}}}"#,
            "a".repeat(usage_events::MAX_BODY_BYTES)
        );
        assert_eq!(
            post(&router, &body, &[]).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::data::account::db_tests::{cleanup, connect, seed_user};
    use crate::data::usage::db_tests::clear;

    /// A session for `user`, returned as its cookie header value.
    async fn session_cookie(pool: &sqlx::PgPool, user: &str) -> String {
        let token = crate::auth::generate_session_token();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, NOW() + INTERVAL '1 hour')",
        )
        .bind(crate::auth::hash_session_token(&token))
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
        format!("{}={token}", crate::auth::SESSION_COOKIE_NAME)
    }

    async fn page_views(pool: &sqlx::PgPool, signed_in: bool) -> i64 {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0)::bigint FROM usage_daily \
             WHERE day = $1 AND event = 'page_view' AND dim = '/cookies' AND signed_in = $2",
        )
        .bind(crate::routes::london_today())
        .bind(signed_in)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn send(router: &axum::Router, cookie: Option<&str>, headers: &[(&str, &str)]) {
        let mut request = Request::builder()
            .method("POST")
            .uri("/public/events")
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = router
            .clone()
            .oneshot(
                request
                    .body(Body::from(
                        r#"{"name":"page_view","route_template":"/cookies"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    /// `/cookies`' page views today, as the handler counts them: anonymous
    /// visitors and opted-in accounts are counted, a signed-in user who has
    /// not opted in is not, nor is anyone sending GPC or DNT. (Uses the
    /// real `/cookies` dim, so it compares before and after.)
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
    async fn events_are_counted_only_with_consent_and_without_a_privacy_signal() {
        let pool = connect().await;
        cleanup(&pool, "usage-route-").await;
        clear(&pool).await;
        seed_user(&pool, "usage-route-in").await;
        seed_user(&pool, "usage-route-out").await;
        usage::set_opt_in(&pool, "usage-route-in", true)
            .await
            .unwrap();
        let opted_in = session_cookie(&pool, "usage-route-in").await;
        let opted_out = session_cookie(&pool, "usage-route-out").await;
        let router = Router::new()
            .nest("/public", router())
            .with_state(crate::test_support::inert_app(pool.clone()));

        let (anonymous_before, signed_in_before) = (
            page_views(&pool, false).await,
            page_views(&pool, true).await,
        );
        send(&router, None, &[]).await;
        send(&router, None, &[]).await;
        send(&router, Some(&opted_in), &[]).await;
        send(&router, Some(&opted_out), &[]).await;
        send(&router, None, &[("sec-gpc", "1")]).await;
        send(&router, Some(&opted_in), &[("dnt", "1")]).await;
        assert_eq!(page_views(&pool, false).await - anonymous_before, 2);
        assert_eq!(page_views(&pool, true).await - signed_in_before, 1);

        // The preference round-trips through the account.
        let put = |cookie: String, opt_in: bool| {
            let router = router.clone();
            async move {
                router
                    .oneshot(
                        Request::builder()
                            .method("PUT")
                            .uri("/public/preferences/analytics")
                            .header("content-type", "application/json")
                            .header("cookie", cookie)
                            .body(Body::from(format!(r#"{{"optIn":{opt_in}}}"#)))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }
        };
        assert_eq!(put(opted_out.clone(), true).await, StatusCode::NO_CONTENT);
        assert_eq!(
            usage::opt_in(&pool, "usage-route-out").await.unwrap(),
            Some(true)
        );
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/public/preferences/analytics")
                    .header("cookie", &opted_out)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], br#"{"optIn":true}"#);
        let anonymous = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/public/preferences/analytics")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"optIn":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        cleanup(&pool, "usage-route-").await;
    }
}
