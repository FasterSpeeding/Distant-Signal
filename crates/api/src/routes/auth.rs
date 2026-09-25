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

/// Whether the `Secure` cookie attribute is appropriate for the browser
/// this app is actually serving. `sso_redirect_url` is the one config
/// value that's already the real, operator-set, browser-facing callback
/// origin (e.g. `http://localhost:3000/...` in local dev, a real
/// `https://...` URL in any deployed environment) -- a `Secure` cookie is
/// unconditionally rejected by the browser when the page isn't served
/// over HTTPS, confirmed live against local dev before this existed (every
/// `Set-Cookie` here used to hardcode `Secure`, so login could never
/// actually set a cookie over plain `http://localhost:3000`).
fn cookie_secure(app: &App) -> bool {
    app.config.sso_redirect_url.starts_with("https://")
}

#[derive(Deserialize)]
struct LoginParams {
    return_to: Option<String>,
}

/// The `return_to` to persist for this login attempt: the captured value
/// if it validates, else `None` -- silently discarded, never a reason to
/// fail the attempt (see this plan's Global Constraints: fallback is
/// always silent).
fn captured_return_to(raw: Option<&str>) -> Option<String> {
    raw.and_then(auth::validate_return_to)
}

async fn login(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    // A hard `Query<LoginParams>` extractor would reject the whole request
    // with a user-visible 400 on a malformed query string (e.g. a
    // duplicate `return_to`), which would abort the OIDC flow before this
    // body even runs -- a genuine regression, and a violation of "fallback
    // is always silent". Extracting as a `Result` instead means axum
    // always calls this handler; a malformed query string just becomes
    // `Err`, handled below identically to "no return_to was sent".
    params: Result<Query<LoginParams>, axum::extract::rejection::QueryRejection>,
) -> Response {
    // Defense in depth against the DB-write churn this route's own
    // `insert_login_state` sweep makes non-free (see that function's doc
    // comment): a genuine top-level navigation here (a user clicking a
    // real "Log in" link) legitimately carries NO `Origin` header at all --
    // browsers only attach `Origin` to a top-level GET navigation in a
    // handful of cross-site cases, unlike a POST, which always carries one
    // -- so this can't use the strict "Origin-or-Referer-required" check
    // `logout` below uses without rejecting real logins. What it CAN
    // reject without any false positives: an `Origin` header that IS
    // present but names a different site entirely, which is exactly the
    // shape a cross-origin `fetch`/XHR (as opposed to a real navigation)
    // hitting this endpoint would carry. Absence of `Origin` is therefore
    // treated as "can't tell, allow" here, not "reject" -- deliberately
    // asymmetric with `logout`'s own check.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && Some(origin) != auth::expected_browser_origin(&app.config.sso_redirect_url).as_deref()
    {
        tracing::warn!(
            origin,
            "login rejected: Origin header names a different site"
        );
        return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
    }

    let (url, pkce_verifier, csrf_state, nonce) = match app.oidc.authorize_url().await {
        Ok(v) => v,
        Err(err) => {
            tracing::error!(error = ?err, "OIDC discovery/authorize_url failed");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "sign-in temporarily unavailable",
            )
                .into_response();
        }
    };

    // An invalid or malicious return_to -- including a query string that
    // failed to deserialize at all -- is discarded at the door and never
    // persisted: insert_login_state receives None for it, exactly as if
    // the parameter had never been sent.
    let return_to = captured_return_to(params.ok().and_then(|Query(p)| p.return_to).as_deref());

    let login_state_id = auth::generate_session_token();
    if let Err(err) = users::insert_login_state(
        &app.database,
        &login_state_id,
        pkce_verifier.secret(),
        nonce.secret(),
        csrf_state.secret(),
        return_to.as_deref(),
    )
    .await
    {
        tracing::error!(error = ?err, "failed to store login state");
        return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
    }

    let mut response = Redirect::temporary(url.as_str()).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::set_cookie_header(
            auth::LOGIN_STATE_COOKIE_NAME,
            &login_state_id,
            900,
            cookie_secure(&app),
        ))
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

/// The post-login redirect target: the stored `return_to` if it still
/// validates (re-checked here, defense in depth), else the operator's
/// static fallback.
fn post_login_target(stored_return_to: Option<&str>, fallback: &str) -> String {
    stored_return_to
        .and_then(auth::validate_return_to)
        .unwrap_or_else(|| fallback.to_string())
}

/// Deliberately carries NO `Origin`/`Referer` check, unlike `login` and
/// `logout` in this same module (2026-09-25 Low-severity auth-core
/// review). The whole point of this route is that the browser arrives
/// here via a cross-origin, top-level GET redirect FROM the IdP
/// (Authentik) -- a legitimate callback's `Referer` therefore names
/// Authentik's own origin, never this app's, and `Origin` is essentially
/// never sent on a top-level GET navigation at all. A same-origin check
/// here would either always fail-reject genuine logins (if compared
/// against this app's own origin) or provide no real signal (if merely
/// checked for presence). This route's actual, and stronger, CSRF defense
/// is already in place: the `state` query parameter Authentik echoes back
/// must match `csrf_state`, a random value bound to the `login_state_id`
/// stored server-side and named only by the `HttpOnly`/`SameSite=Lax`
/// login-state cookie set by `login` above (see the `stored.csrf_state !=
/// state` check below) -- functionally the OAuth2 spec's own standard
/// answer to this exact CSRF concern, and strictly harder to forge than an
/// `Origin` header (which some HTTP clients let a caller set arbitrarily)
/// would add on top of it.
async fn callback(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Response {
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
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                "login state expired or already used",
            )
                .into_response();
        }
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
    // Re-validate stored.return_to again here (defense in depth -- cheap,
    // and guards against any future code path that might write to that
    // column without going through login()'s own validation), not just
    // trust that it was already validated once at insert time.
    let target = post_login_target(
        stored.return_to.as_deref(),
        &app.config.sso_post_login_redirect_url,
    );
    let mut response = Redirect::temporary(&target).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::set_cookie_header(
            auth::SESSION_COOKIE_NAME,
            &session_token,
            max_age,
            cookie_secure(&app),
        ))
        .expect("cookie header value is always valid ASCII"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(
            auth::LOGIN_STATE_COOKIE_NAME,
            cookie_secure(&app),
        ))
        .expect("cookie header value is always valid ASCII"),
    );
    response
}

async fn logout(State(app): State<App>, headers: axum::http::HeaderMap) -> Response {
    // Belt-and-suspenders CSRF defense in depth (2026-09-25 Low-severity
    // auth-core review), on top of the existing `SameSite=Lax` posture
    // `main.rs`'s CORS comment already relies on: `logout` is a `POST`
    // that acts on whatever session cookie the browser happens to attach,
    // exactly the shape a CSRF-vulnerable endpoint takes if `SameSite`
    // alone (enforced entirely client-side, nothing backing it up
    // server-side) is its only guard. Mirrors
    // `frontend/app/connect-claude/authorize/route.ts`'s own
    // `isSameOriginRequest` -- the strict form (unlike `login` above): a
    // real POST always carries an `Origin` or `Referer` header, so
    // rejecting when neither is present has no legitimate-request cost
    // here the way it would on a plain top-level GET navigation.
    let expected_origin = auth::expected_browser_origin(&app.config.sso_redirect_url);
    if let Some(expected_origin) = expected_origin.as_deref()
        && !auth::is_same_origin(
            headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()),
            headers.get(header::REFERER).and_then(|v| v.to_str().ok()),
            expected_origin,
        )
    {
        tracing::warn!("logout rejected: Origin/Referer did not match this app's own origin");
        return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
    }

    // Local-only logout -- this plan does not implement RP-Initiated
    // Logout (see Global Constraints). If the session cookie is missing
    // or already invalid, logout is still a no-op success (idempotent),
    // not an error.
    if let Some(token) = auth::parse_cookie(&headers, auth::SESSION_COOKIE_NAME)
        && let Err(err) =
            users::delete_session(&app.database, &auth::hash_session_token(&token)).await
    {
        tracing::error!(error = ?err, "failed to delete session on logout");
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(
            auth::SESSION_COOKIE_NAME,
            cookie_secure(&app),
        ))
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
    /// Always present, empty when logged out or when the logged-in user
    /// asserted no groups -- never omitted, so a consumer (Task 9's
    /// adapter, in distant-signal-mcp's own separate repository) can
    /// always treat this as a plain string array rather than an optional
    /// field. See
    /// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
    /// Decision 2's "one existing read-back point" framing.
    groups: Vec<String>,
}

async fn session(
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Json<SessionResponse> {
    match user {
        Some(u) => Json(SessionResponse {
            authenticated: true,
            id: Some(u.id),
            email: u.email,
            name: u.name,
            groups: u.groups,
        }),
        None => Json(SessionResponse {
            authenticated: false,
            id: None,
            email: None,
            name: None,
            groups: vec![],
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_return_to_discards_an_invalid_return_to_rather_than_erroring() {
        assert_eq!(captured_return_to(Some("https://evil.com")), None);
    }

    #[test]
    fn captured_return_to_keeps_a_valid_return_to() {
        assert_eq!(
            captured_return_to(Some("/lines/some-line")),
            Some("/lines/some-line".to_string())
        );
    }

    #[test]
    fn captured_return_to_treats_no_return_to_the_same_as_an_invalid_one() {
        assert_eq!(captured_return_to(None), None);
    }

    #[test]
    fn post_login_target_uses_the_stored_return_to_when_valid() {
        let target = post_login_target(
            Some("/lines/some-line?tab=history"),
            "https://rail.example.com/",
        );
        assert_eq!(target, "/lines/some-line?tab=history");
    }

    #[test]
    fn post_login_target_falls_back_when_return_to_is_none() {
        let fallback = "https://rail.example.com/";
        assert_eq!(post_login_target(None, fallback), fallback);
    }

    #[test]
    fn post_login_target_falls_back_when_the_stored_return_to_fails_revalidation() {
        // Defense-in-depth case: a stored value that, hypothetically, didn't
        // go through login()'s own validation (e.g. a future code path with a
        // bug) must still be caught here, not trusted blindly.
        let fallback = "https://rail.example.com/";
        assert_eq!(
            post_login_target(Some("https://evil.com"), fallback),
            fallback
        );
    }

    #[test]
    fn session_response_serializes_groups_as_a_plain_camel_case_array() {
        let response = SessionResponse {
            authenticated: true,
            id: Some("user-123".to_string()),
            email: Some("rider@example.com".to_string()),
            name: Some("Ada Rider".to_string()),
            groups: vec!["mcp-users".to_string(), "mcp-live-boards".to_string()],
        };
        let json = serde_json::to_value(&response).expect("serializes");
        assert_eq!(
            json["groups"],
            serde_json::json!(["mcp-users", "mcp-live-boards"])
        );
    }

    #[test]
    fn session_response_groups_is_an_empty_array_not_null_when_logged_out() {
        let response = SessionResponse {
            authenticated: false,
            id: None,
            email: None,
            name: None,
            groups: vec![],
        };
        let json = serde_json::to_value(&response).expect("serializes");
        assert_eq!(json["groups"], serde_json::json!([]));
    }
}
