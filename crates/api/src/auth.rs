//! Internal-auth gate for `private_router()`.
//!
//! One shared-secret header (`X-Internal-Token`), compared in fixed time
//! against `ServiceArguments::internal_token`. This is intentionally not a
//! general auth framework — just enough to keep the ingestion endpoints
//! from being reachable by anyone who can hit the API's port.

pub mod oidc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderMap, StatusCode, request::Parts};
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

pub const SESSION_COOKIE_NAME: &str = "distant_signal_session";
pub const LOGIN_STATE_COOKIE_NAME: &str = "distant_signal_login";

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

/// `secure` MUST be the browser-facing origin's actual scheme, not a fixed
/// `true` -- a `Secure` cookie is unconditionally rejected by the browser
/// over plain HTTP (confirmed live: local dev, served over
/// `http://localhost:3000`, could never actually receive either cookie
/// this app sets, since every `Set-Cookie` unconditionally carried
/// `Secure`). Callers derive this from `sso_redirect_url`'s scheme -- see
/// `routes/auth.rs`'s `cookie_secure` -- since that's the one config value
/// that's already the real, operator-configured, browser-facing origin
/// this app is served from in any given environment.
pub fn set_cookie_header(name: &str, value: &str, max_age_secs: i64, secure: bool) -> String {
    let secure = if secure { "Secure; " } else { "" };
    format!("{name}={value}; Path=/; HttpOnly; {secure}SameSite=Lax; Max-Age={max_age_secs}")
}

pub fn clear_cookie_header(name: &str, secure: bool) -> String {
    let secure = if secure { "Secure; " } else { "" };
    format!("{name}=; Path=/; HttpOnly; {secure}SameSite=Lax; Max-Age=0")
}

/// Accepts only a same-origin, absolute-path, relative URL reference --
/// rejects anything that could make `Redirect::temporary` send a
/// just-authenticated, trusting browser somewhere off-site (open
/// redirect / post-login phishing). Called twice per login: once in
/// `routes::auth::login` (validate before persisting to
/// `oidc_login_state`) and once in `routes::auth::callback` (validate
/// again before using the persisted value) -- see that module for both
/// call sites.
pub fn validate_return_to(raw: &str) -> Option<String> {
    const MAX_LEN: usize = 2048;
    if raw.is_empty() || raw.len() > MAX_LEN {
        return None;
    }
    // Header-injection guard, and a defense against browsers that strip
    // or reinterpret stray control characters (tabs, NULs) during URL
    // normalization in ways this function shouldn't have to model.
    if raw.chars().any(|c| c.is_control()) {
        return None;
    }
    // Some browsers normalize a leading `/\` (or backslashes generally)
    // into `//` during navigation -- i.e. into a protocol-relative URL.
    // Rejecting `\` anywhere sidesteps needing to reason about exactly
    // which browsers do this and how.
    if raw.contains('\\') {
        return None;
    }
    // Must be an absolute-path reference: exactly one leading '/', not
    // '//...' (protocol-relative -- a browser resolves this to
    // `https://<attacker-controlled-host>/...`) and not a scheme
    // (`javascript:`, `https:`, etc., which `starts_with('/')` already
    // excludes on its own, but is worth stating as intent).
    if !raw.starts_with('/') || raw.starts_with("//") {
        return None;
    }
    // Authoritative check, not just belt-and-braces: resolve `raw`
    // against a fixed, arbitrary dummy origin using the same URL parser
    // this crate already depends on (`openidconnect::url`, i.e. the
    // `url` crate -- a WHATWG URL Standard implementation, the same
    // parsing algorithm real browsers use). If the parsed result's
    // scheme/host ever differ from the dummy origin, `raw` smuggled a
    // scheme or host past the prefix checks above through some
    // normalization quirk those checks didn't anticipate -- reject
    // rather than trust the prefix checks alone.
    let base = openidconnect::url::Url::parse("http://return-to.invalid").ok()?;
    let joined = base.join(raw).ok()?;
    if joined.scheme() != "http" || joined.host_str() != Some("return-to.invalid") {
        return None;
    }
    Some(raw.to_string())
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
        let token = parse_cookie(&parts.headers, SESSION_COOKIE_NAME).ok_or((
            axum::http::StatusCode::UNAUTHORIZED,
            "no session".to_string(),
        ))?;
        let hashed = hash_session_token(&token);
        let session = crate::data::users::get_session_with_user(&app.database, &hashed)
            .await
            .map_err(|err| {
                tracing::error!(error = ?err, "session lookup failed");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "session lookup failed".to_string(),
                )
            })?
            .ok_or((
                axum::http::StatusCode::UNAUTHORIZED,
                "session expired or unknown".to_string(),
            ))?;
        Ok(AuthenticatedUser {
            id: session.id,
            email: session.email,
            name: session.name,
        })
    }
}

/// Same lookup as `AuthenticatedUser`, but never rejects -- `None` for "no
/// session" instead of `401`. Used only by `GET /auth/session` (Task 7),
/// which must report "not logged in" as a normal `200`, not an error.
pub struct OptionalAuthenticatedUser(pub Option<AuthenticatedUser>);

impl FromRequestParts<App> for OptionalAuthenticatedUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuthenticatedUser(
            AuthenticatedUser::from_request_parts(parts, app).await.ok(),
        ))
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
        headers.insert(
            axum::http::header::COOKIE,
            "distant_signal_session=abc123".parse().unwrap(),
        );
        assert_eq!(
            parse_cookie(&headers, "distant_signal_session"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn parse_cookie_finds_one_among_several() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "theme=dark; distant_signal_session=abc123; other=x"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            parse_cookie(&headers, "distant_signal_session"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn parse_cookie_returns_none_when_absent() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "theme=dark".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "distant_signal_session"), None);
    }

    #[test]
    fn parse_cookie_returns_none_with_no_cookie_header_at_all() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(parse_cookie(&headers, "distant_signal_session"), None);
    }

    #[test]
    fn set_cookie_header_includes_all_required_attributes_when_secure() {
        let header = set_cookie_header("distant_signal_session", "abc123", 1_209_600, true);
        assert!(header.starts_with("distant_signal_session=abc123;"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("Secure"));
        assert!(header.contains("SameSite=Lax"));
        assert!(header.contains("Max-Age=1209600"));
        assert!(header.contains("Path=/"));
    }

    #[test]
    fn set_cookie_header_omits_secure_over_plain_http() {
        // A `Secure` cookie is unconditionally rejected by the browser over
        // plain HTTP -- this is the live bug this parameter fixes.
        let header = set_cookie_header("distant_signal_session", "abc123", 1_209_600, false);
        assert!(!header.contains("Secure"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Lax"));
    }

    #[test]
    fn clear_cookie_header_zeroes_max_age() {
        let header = clear_cookie_header("distant_signal_session", true);
        assert!(header.starts_with("distant_signal_session=;"));
        assert!(header.contains("Max-Age=0"));
        assert!(header.contains("Secure"));
    }

    #[test]
    fn clear_cookie_header_omits_secure_over_plain_http() {
        let header = clear_cookie_header("distant_signal_session", false);
        assert!(!header.contains("Secure"));
    }

    #[test]
    fn hash_session_token_is_deterministic() {
        assert_eq!(
            hash_session_token("same-token"),
            hash_session_token("same-token")
        );
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

    #[test]
    fn validate_return_to_accepts_a_plain_relative_path() {
        assert_eq!(
            validate_return_to("/lines/some-line"),
            Some("/lines/some-line".to_string())
        );
    }

    #[test]
    fn validate_return_to_accepts_a_relative_path_with_a_query_string_unchanged() {
        // Returns the ORIGINAL string, not a re-serialization -- Url's own
        // serialization can reorder/re-encode a query string in ways a
        // caller wouldn't expect.
        let raw = "/lines/some-line?tab=history&x=1";
        assert_eq!(validate_return_to(raw), Some(raw.to_string()));
    }

    #[test]
    fn validate_return_to_rejects_empty_string() {
        assert_eq!(validate_return_to(""), None);
    }

    #[test]
    fn validate_return_to_rejects_oversized_input() {
        let raw = format!("/{}", "a".repeat(2048));
        assert_eq!(validate_return_to(&raw), None);
    }

    #[test]
    fn validate_return_to_accepts_input_at_exactly_the_length_cap() {
        let raw = format!("/{}", "a".repeat(2047)); // total length 2048
        assert!(validate_return_to(&raw).is_some());
    }

    #[test]
    fn validate_return_to_rejects_control_characters() {
        assert_eq!(validate_return_to("/foo\tbar"), None);
        assert_eq!(validate_return_to("/foo\r\nbar"), None);
        assert_eq!(validate_return_to("/foo\0bar"), None);
    }

    #[test]
    fn validate_return_to_rejects_backslash_tricks() {
        // Some browsers normalize a leading /\ into // (protocol-relative)
        // during navigation.
        assert_eq!(validate_return_to("/\\evil.com"), None);
        assert_eq!(validate_return_to("/foo\\bar"), None);
    }

    #[test]
    fn validate_return_to_rejects_protocol_relative_urls() {
        assert_eq!(validate_return_to("//evil.com"), None);
        assert_eq!(validate_return_to("//evil.com/path"), None);
    }

    #[test]
    fn validate_return_to_rejects_absolute_urls_with_a_scheme_and_host() {
        assert_eq!(validate_return_to("https://evil.com/phish"), None);
        assert_eq!(validate_return_to("http://evil.com"), None);
    }

    #[test]
    fn validate_return_to_rejects_a_javascript_scheme() {
        assert_eq!(validate_return_to("javascript:alert(1)"), None);
    }

    #[test]
    fn validate_return_to_rejects_fragment_only_input() {
        // A fragment is never sent to the server on any HTTP request -- this
        // isn't a bypass, it's this function correctly rejecting a value that
        // was never a valid absolute-path reference to begin with (no leading
        // '/'). Documents the known, accepted limitation from the spec's Open
        // Questions: LoginLink has no mechanism to round-trip a URL fragment
        // through this flow at all.
        assert_eq!(validate_return_to("#section"), None);
    }

    #[test]
    fn validate_return_to_rejects_a_bare_double_slash_with_no_path() {
        assert_eq!(validate_return_to("//"), None);
    }

    #[test]
    fn validate_return_to_currently_accepts_a_return_path_back_into_the_auth_flow_itself() {
        // NOT a security hole -- these are same-origin absolute-path
        // references, which is all this function verifies -- but a plausible
        // dead-end/confusing-loop edge case the design spec's Open Questions
        // section explicitly flags and does NOT resolve in this pass (no
        // redirect-loop guard is implemented -- see this plan's Global
        // Constraints). This test pins today's actual behavior so that
        // whoever eventually adds the guard gets a failing test forcing them
        // to update it, rather than a silent behavior change.
        assert!(validate_return_to("/api/auth/login").is_some());
        assert!(validate_return_to("/api/auth/callback").is_some());
    }
}
