//! Internal-auth gate for `private_router()`.
//!
//! One shared-secret header (`X-Internal-Token`), compared in fixed time
//! against `ServiceArguments::internal_token`. This is intentionally not a
//! general auth framework — just enough to keep the ingestion endpoints
//! from being reachable by anyone who can hit the API's port.

pub mod oidc;

use axum::extract::{Request, State, FromRequestParts};
use axum::http::{StatusCode, HeaderMap, request::Parts};
use axum::middleware::Next;
use axum::response::Response;
use common::ingest::INTERNAL_TOKEN_HEADER;

use crate::app::App;

/// `axum::middleware::from_fn` handler enforcing the shared-secret header.
/// Applied only to `private_router()` — `public_router()` never sees this.
pub async fn require_internal_token(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let provided = request
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if constant_time_eq(provided.as_bytes(), app.config.internal_token.as_bytes()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Fixed-time byte comparison: no early return based on *content*, so a
/// mismatching byte doesn't short-circuit the scan. (A length mismatch is
/// still rejected immediately — hiding token *length* isn't a goal here,
/// only avoiding a byte-by-byte timing oracle on a same-length guess.)
///
/// Hand-rolled rather than pulling in the `subtle` crate: this is a single
/// comparison in one call site, and `subtle::ConstantTimeEq` has the same
/// same-length requirement, so there's no behavioral gap being traded away.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const SESSION_COOKIE_NAME: &str = "nr_session";
pub const LOGIN_STATE_COOKIE_NAME: &str = "nr_login";

/// Parses a `Cookie` request header for one named value. Hand-rolled
/// rather than pulling in `axum-extra`'s `CookieJar` -- this app needs
/// exactly "read one cookie by name" and "build one Set-Cookie value",
/// both single-call-site jobs, matching this file's existing
/// `constant_time_eq` precedent for hand-rolling something this narrow
/// rather than adding a dependency for it.
pub fn parse_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

pub fn set_cookie_header(name: &str, value: &str, max_age_secs: i64) -> String {
    format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age_secs}")
}

pub fn clear_cookie_header(name: &str) -> String {
    format!("{name}=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0")
}

/// A fresh, high-entropy opaque session/login-state token: 256 bits of OS
/// randomness, base64url-encoded (no padding) for a clean cookie value.
/// This is the value actually sent to the browser -- never stored
/// verbatim (see `hash_session_token`).
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `sessions.id` stores this, not the raw token -- mirrors how a password
/// hash works: a DB dump/leak alone can't be replayed as a live session
/// cookie, only the original random token can. Resolves design doc Open
/// Question 4 in favor of its own stated "more defensible default."
pub fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// A resolved, authenticated user -- the `axum` extractor every
/// ownership-scoped handler (custom-line mutations, pinned-lines/
/// pinned-stations reads and writes -- Tasks 9/10) depends on instead of
/// `State<App>` alone. Rejects with `401` if there's no session cookie, no
/// matching (unexpired) `sessions` row, or the row's user was deleted out
/// from under it.
pub struct AuthenticatedUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

impl FromRequestParts<App> for AuthenticatedUser {
    type Rejection = (axum::http::StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = parse_cookie(&parts.headers, SESSION_COOKIE_NAME)
            .ok_or((axum::http::StatusCode::UNAUTHORIZED, "no session".to_string()))?;
        let hashed = hash_session_token(&token);
        let session = crate::data::users::get_session_with_user(&app.database, &hashed)
            .await
            .map_err(|err| {
                tracing::error!(error = ?err, "session lookup failed");
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "session lookup failed".to_string())
            })?
            .ok_or((axum::http::StatusCode::UNAUTHORIZED, "session expired or unknown".to_string()))?;
        Ok(AuthenticatedUser { id: session.id, email: session.email, name: session.name })
    }
}

/// Same lookup as `AuthenticatedUser`, but never rejects -- `None` for "no
/// session" instead of `401`. Used only by `GET /auth/session` (Task 7),
/// which must report "not logged in" as a normal `200`, not an error.
pub struct OptionalAuthenticatedUser(pub Option<AuthenticatedUser>);

impl FromRequestParts<App> for OptionalAuthenticatedUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuthenticatedUser(AuthenticatedUser::from_request_parts(parts, app).await.ok()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_tokens_match() {
        assert!(constant_time_eq(b"super-secret", b"super-secret"));
    }

    #[test]
    fn different_content_same_length_does_not_match() {
        assert!(!constant_time_eq(b"super-secret", b"super-sekret"));
    }

    #[test]
    fn different_length_does_not_match() {
        assert!(!constant_time_eq(b"short", b"much-longer-token"));
    }

    #[test]
    fn empty_tokens_match() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn empty_provided_against_real_token_does_not_match() {
        assert!(!constant_time_eq(b"", b"super-secret"));
    }

    #[test]
    fn parse_cookie_finds_a_single_named_cookie() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "nr_session=abc123".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "nr_session"), Some("abc123".to_string()));
    }

    #[test]
    fn parse_cookie_finds_one_among_several() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "theme=dark; nr_session=abc123; other=x".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "nr_session"), Some("abc123".to_string()));
    }

    #[test]
    fn parse_cookie_returns_none_when_absent() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "theme=dark".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "nr_session"), None);
    }

    #[test]
    fn parse_cookie_returns_none_with_no_cookie_header_at_all() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(parse_cookie(&headers, "nr_session"), None);
    }

    #[test]
    fn set_cookie_header_includes_all_required_attributes() {
        let header = set_cookie_header("nr_session", "abc123", 1_209_600);
        assert!(header.starts_with("nr_session=abc123;"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("Secure"));
        assert!(header.contains("SameSite=Lax"));
        assert!(header.contains("Max-Age=1209600"));
        assert!(header.contains("Path=/"));
    }

    #[test]
    fn clear_cookie_header_zeroes_max_age() {
        let header = clear_cookie_header("nr_session");
        assert!(header.starts_with("nr_session=;"));
        assert!(header.contains("Max-Age=0"));
    }

    #[test]
    fn hash_session_token_is_deterministic() {
        assert_eq!(hash_session_token("same-token"), hash_session_token("same-token"));
    }

    #[test]
    fn hash_session_token_differs_for_different_tokens() {
        assert_ne!(hash_session_token("token-a"), hash_session_token("token-b"));
    }

    #[test]
    fn generated_session_tokens_are_not_repeated() {
        // Not a proof of randomness, just a smoke test that two calls
        // don't collide -- a collision here would indicate
        // generate_session_token is broken (e.g. always returning a fixed
        // value), not bad luck.
        assert_ne!(generate_session_token(), generate_session_token());
    }
}
