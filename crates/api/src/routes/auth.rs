//! `/public/auth/...`: OIDC login/callback/logout and session-status
//! check. Mounted under `/public` so the existing Next.js proxy forwards
//! `/api/auth/*` unmodified -- see
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Auth
//! routes section and Task 8's proxy fix (required for the redirects and
//! cookies this module issues to actually reach the browser).

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{self, OptionalAuthenticatedUser};
use crate::data::users;

pub fn router() -> Router {
    Router::new()
        .route("/auth/login", axum::routing::get(login))
        .route("/auth/callback", axum::routing::get(callback))
        .route("/auth/logout", axum::routing::post(logout))
        .route("/auth/session", axum::routing::get(session))
}

async fn login(State(app): State<App>) -> Response {
    let (url, pkce_verifier, csrf_state, nonce) = match app.oidc.authorize_url().await {
        Ok(v) => v,
        Err(err) => {
            tracing::error!(error = ?err, "OIDC discovery/authorize_url failed");
            return (StatusCode::SERVICE_UNAVAILABLE, "sign-in temporarily unavailable").into_response();
        }
    };

    let login_state_id = auth::generate_session_token();
    if let Err(err) = users::insert_login_state(
        &app.database,
        &login_state_id,
        pkce_verifier.secret(),
        nonce.secret(),
        csrf_state.secret(),
    )
    .await
    {
        tracing::error!(error = ?err, "failed to store login state");
        return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
    }

    let mut response = Redirect::temporary(url.as_str()).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::set_cookie_header(auth::LOGIN_STATE_COOKIE_NAME, &login_state_id, 900))
            .expect("cookie header value is always valid ASCII"),
    );
    response
}

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(State(app): State<App>, headers: axum::http::HeaderMap, Query(params): Query<CallbackParams>) -> Response {
    if let Some(error) = params.error {
        tracing::warn!(oidc_error = %error, "SSO server returned an error to the callback");
        return (StatusCode::BAD_GATEWAY, "sign-in was not completed").into_response();
    }
    let (Some(code), Some(state)) = (params.code, params.state) else {
        return (StatusCode::BAD_REQUEST, "missing code or state").into_response();
    };

    let Some(login_state_id) = auth::parse_cookie(&headers, auth::LOGIN_STATE_COOKIE_NAME) else {
        return (StatusCode::BAD_REQUEST, "missing login state cookie").into_response();
    };
    let stored = match users::consume_login_state(&app.database, &login_state_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return (StatusCode::BAD_REQUEST, "login state expired or already used").into_response(),
        Err(err) => {
            tracing::error!(error = ?err, "login state lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
        }
    };
    if stored.csrf_state != state {
        tracing::warn!("OIDC callback state mismatch -- possible CSRF attempt or stale link");
        return (StatusCode::BAD_REQUEST, "state mismatch").into_response();
    }

    let exchange_result = app
        .oidc
        .exchange_code(
            code,
            openidconnect::PkceCodeVerifier::new(stored.pkce_verifier),
            &openidconnect::Nonce::new(stored.nonce),
        )
        .await;
    // The refresh token is deliberately dropped rather than persisted:
    // nothing in this plan consumes one (no silent renewal), and
    // `users::insert_session` explains why storing an unused live IdP
    // credential is not worth the blast radius. `exchange_code` still
    // surfaces it so the eventual refresh work has nothing to re-plumb.
    let (identity, _refresh_token) = match exchange_result {
        Ok(result) => result,
        Err(err) => {
            tracing::warn!(error = ?err, "OIDC code exchange failed");
            return (StatusCode::BAD_GATEWAY, "sign-in failed").into_response();
        }
    };

    let user = match users::upsert_user(&app.database, &identity).await {
        Ok(u) => u,
        Err(err) => {
            tracing::error!(error = ?err, "failed to upsert user");
            return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
        }
    };

    let session_token = auth::generate_session_token();
    let insert_result = users::insert_session(
        &app.database,
        &auth::hash_session_token(&session_token),
        &user.id,
        app.config.session_ttl_days,
    )
    .await;
    if let Err(err) = insert_result {
        tracing::error!(error = ?err, "failed to create session");
        return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
    }

    let max_age = app.config.session_ttl_days * 24 * 60 * 60;
    let mut response = Redirect::temporary(&app.config.sso_post_login_redirect_url).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::set_cookie_header(auth::SESSION_COOKIE_NAME, &session_token, max_age))
            .expect("cookie header value is always valid ASCII"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(auth::LOGIN_STATE_COOKIE_NAME))
            .expect("cookie header value is always valid ASCII"),
    );
    response
}

async fn logout(State(app): State<App>, headers: axum::http::HeaderMap) -> Response {
    // Local-only logout -- this plan does not implement RP-Initiated
    // Logout (see Global Constraints). If the session cookie is missing
    // or already invalid, logout is still a no-op success (idempotent),
    // not an error.
    if let Some(token) = auth::parse_cookie(&headers, auth::SESSION_COOKIE_NAME)
        && let Err(err) = users::delete_session(&app.database, &auth::hash_session_token(&token)).await
    {
        tracing::error!(error = ?err, "failed to delete session on logout");
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(auth::SESSION_COOKIE_NAME))
            .expect("cookie header value is always valid ASCII"),
    );
    response
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    authenticated: bool,
    id: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

async fn session(OptionalAuthenticatedUser(user): OptionalAuthenticatedUser) -> Json<SessionResponse> {
    match user {
        Some(u) => Json(SessionResponse { authenticated: true, id: Some(u.id), email: u.email, name: u.name }),
        None => Json(SessionResponse { authenticated: false, id: None, email: None, name: None }),
    }
}
