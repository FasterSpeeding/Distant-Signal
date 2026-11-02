//! Authentication: browser sessions (OIDC login, session cookies, the
//! same-origin check) and the internal-service bearer token.
//!
//! The internal `OAuth2` client-credentials token (RFC 6750/6749 §4.4,
//! issued by Authentik, verified locally against its JWKS by
//! `internal_oauth::ServiceTokenVerifier`) gates no route any more: the
//! `/private` ingest routes it guarded were removed in release B (ingest
//! phase 5, docs/ingest-phase5-runbook.md). It now only identifies the
//! Distant-Signal-MCP, so `rate_limit::ServiceTokenCheck` can give it its
//! own budget.

pub mod backchannel_logout;
pub mod delegated;
pub mod internal_oauth;
pub mod oidc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderMap, StatusCode, request::Parts};
use axum::middleware::Next;
use axum::response::Response;

use crate::app::App;

/// The bearer token of an `Authorization: Bearer` header, if any: the
/// Distant-Signal-MCP's service token, which `rate_limit::ServiceTokenCheck`
/// verifies to give the MCP its own budget.
pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    value.strip_prefix("Bearer ").map(str::to_string)
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
    if raw.chars().any(char::is_control) {
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

/// This app's own real, browser-facing origin (scheme + host[+port]), for
/// comparing against an incoming request's `Origin`/`Referer` header (see
/// `is_same_origin` below). Derived from `sso_redirect_url`, the same
/// config value `routes::auth::cookie_secure` already derives `Secure`
/// from -- see that function's own doc comment for why it's the one config
/// value that's already the real, operator-configured, browser-facing
/// origin this app is served from in any given environment (as opposed to,
/// say, this service's own bind address, which is never what a browser's
/// address bar shows).
pub fn expected_browser_origin(sso_redirect_url: &str) -> Option<String> {
    openidconnect::url::Url::parse(sso_redirect_url)
        .ok()
        .map(|url| url.origin().ascii_serialization())
}

/// Standard OWASP-recommended Origin check for a state-changing request,
/// mirroring the `isSameOriginRequest` of the frontend's MCP consent bridge
/// (`frontend/app/connect-claude/authorize/route.ts`, retired 2026-09-29).
/// The rationale this codebase committed to there: `SameSite=Lax` is this
/// app's only other CSRF precedent, and it's enforced entirely client-side
/// (by the browser) with nothing backing it up server-side.
/// Checks `origin` first; falls back to `referer` only when `origin` is
/// absent (a real browser-submitted POST -- or any genuine `fetch`/XHR --
/// always carries at least one of the two); returns `false` when neither
/// is present rather than guessing. `referer` is a full URL, so only its
/// origin component (scheme+host+port) is compared, exactly like the
/// TypeScript version does via `new URL(referer).origin`.
///
/// NOT applied to `GET /auth/callback` -- see that handler's own doc
/// comment in `routes::auth` for why a same-origin check does not fit that
/// route's shape at all (the whole point of a callback is that the browser
/// arrives there via a cross-origin redirect FROM the `IdP`, so `Referer`
/// legitimately names the `IdP`'s own origin, never this app's).
pub fn is_same_origin(origin: Option<&str>, referer: Option<&str>, expected_origin: &str) -> bool {
    if let Some(origin) = origin {
        return origin == expected_origin;
    }
    if let Some(referer) = referer {
        return openidconnect::url::Url::parse(referer)
            .is_ok_and(|url| url.origin().ascii_serialization() == expected_origin);
    }
    false
}

/// Is this a session-cookie-bearing, state-changing request whose
/// browser-supplied `Origin` (or, absent that, `Referer`) names some other
/// origin than this app's own? See [`reject_cross_origin_cookie_mutation`].
///
/// Deliberately LENIENT on absence, unlike the strict [`is_same_origin`]
/// check `logout` uses: a request carrying neither header is let through,
/// because the frontend's own server-side fetches (`lib/api.ts`) and
/// non-browser clients never send one, while a browser always sends
/// `Origin` on a cross-origin POST/PUT/PATCH/DELETE -- which is the only
/// shape a CSRF attempt can take. Requests with no session cookie are not
/// CSRF-relevant (nothing ambient to ride on) and are ignored, as is every
/// safe method.
pub fn is_cross_origin_cookie_mutation(
    method: &axum::http::Method,
    headers: &HeaderMap,
    expected_origin: &str,
) -> bool {
    use axum::http::Method;
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return false;
    }
    if parse_cookie(headers, SESSION_COOKIE_NAME).is_none() {
        return false;
    }
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    let referer = headers
        .get(axum::http::header::REFERER)
        .and_then(|v| v.to_str().ok());
    if origin.is_none() && referer.is_none() {
        return false;
    }
    !is_same_origin(origin, referer, expected_origin)
}

/// Router-wide api-layer CSRF guard for cookie-authenticated mutations
/// (2026-09-26 "Repeater Signal" review, L4).
///
/// Several session-authenticated routes -- group promote/demote, group
/// join, invite-link create/revoke, journey share-link create/revoke -- are
/// POST/DELETE with no body, i.e. CORS "simple requests" that never
/// preflight, so `main.rs`'s CORS layer cannot stop them (see its comment).
/// Before this, the only api-side barrier was the session cookie's
/// `SameSite=Lax`, which doesn't cover a same-SITE sibling subdomain. The
/// frontend proxy's `hasAcceptableOriginForMutation` covers requests that go
/// through it, but nothing covered the api when it's reachable directly
/// (`ingress.api.enabled`). This rejects any such request whose Origin/
/// Referer names another origin with 403, for every route at once. The
/// proxy forwards the browser's Origin/Referer verbatim, so proxied
/// same-origin requests pass unchanged.
///
/// `expected_origin` is [`expected_browser_origin`] of `SSO_REDIRECT_URL`
/// -- the same value `logout` compares against. `None` (unparseable
/// config) disables the guard rather than rejecting everything, matching
/// `logout`'s own behaviour.
pub async fn reject_cross_origin_cookie_mutation(
    State(expected_origin): State<Option<std::sync::Arc<str>>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(expected_origin) = expected_origin.as_deref()
        && is_cross_origin_cookie_mutation(request.method(), request.headers(), expected_origin)
    {
        tracing::warn!(
            method = %request.method(),
            "rejected a session-cookie mutation whose Origin/Referer names another origin"
        );
        return axum::response::IntoResponse::into_response((
            StatusCode::FORBIDDEN,
            "cross-origin request rejected",
        ));
    }
    next.run(request).await
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
    pub name: Option<String>,
    /// The `preferred_username` claim as of the last sign-in. Self-view
    /// only (`GET /auth/session`); never shown to other users from here.
    pub username: Option<String>,
    pub groups: Vec<String>,
    /// Whether the user has turned on usage analytics (DS-190): read with
    /// the session, so `POST /public/events` and `GET /auth/session` need
    /// no second query.
    pub analytics_opt_in: bool,
}

impl FromRequestParts<App> for AuthenticatedUser {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = parse_cookie(&parts.headers, SESSION_COOKIE_NAME)
            .ok_or((StatusCode::UNAUTHORIZED, "no session".to_string()))?;
        let hashed = hash_session_token(&token);
        let session = crate::data::users::get_session_with_user(&app.database, &hashed)
            .await
            .map_err(|err| {
                if let Some(unavailable) = crate::unavailable::response_for(&err) {
                    return unavailable;
                }
                tracing::error!(error = ?err, "session lookup failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "session lookup failed".to_string(),
                )
            })?
            .ok_or((
                StatusCode::UNAUTHORIZED,
                "session expired or unknown".to_string(),
            ))?;
        slide_session_expiry(app, &hashed, &session).await;
        Ok(AuthenticatedUser {
            id: session.id,
            name: session.name,
            username: session.username,
            groups: session.groups,
            analytics_opt_in: session.analytics_opt_in,
        })
    }
}

/// The sliding half of the session lifetime (`data::users::SessionLifetime`):
/// moves a valid session's expiry forward when it is due, at most once per
/// `session_extend_throttle_hours`. Best-effort: the session was already
/// valid for this request, so a failed write is logged and the request goes
/// on; the next due request tries again.
///
/// The cookie isn't re-issued here: its `Max-Age` is already the absolute
/// cap (see `routes::auth::session_cookie_max_age_secs`), which the sliding
/// expiry never passes, so there is nothing for the browser to learn.
async fn slide_session_expiry(app: &App, hashed: &str, session: &crate::data::users::SessionUser) {
    let lifetime = crate::data::users::SessionLifetime::from_config(&app.config);
    if !lifetime.extension_due(
        session.session_created_at,
        session.session_expires_at,
        session.checked_at,
    ) {
        return;
    }
    if let Err(err) = crate::data::users::extend_session(&app.database, hashed, lifetime).await {
        tracing::warn!(error = ?err, "failed to slide the session expiry; the session stays valid");
    }
}

/// Same lookup as `AuthenticatedUser`, but doesn't turn "no session" into a
/// hard rejection -- `None` for that expected case instead of `401`. Used
/// only by `GET /auth/session` (Task 7), which must report "not logged in"
/// as a normal `200`, not an error.
///
/// Deliberately NOT `Rejection = Infallible` (as an earlier version of this
/// impl was): that collapsed `AuthenticatedUser::from_request_parts`'s TWO
/// distinct failure shapes -- `401` ("no cookie, or a cookie naming no
/// live session row", genuinely anonymous) and `500` ("the session lookup
/// itself errored -- a transient Postgres blip, a pool exhaustion, etc.",
/// NOT anonymous, just unknown") -- into the same `None`. A real logged-in
/// user hitting a transient DB error would silently read back as "not
/// logged in" with no error signal at all, which a frontend then has every
/// reason to treat as "go log in again" -- a silent login loop with
/// nothing in the response to explain why. `Rejection` is now the same
/// `(StatusCode, String)` shape `AuthenticatedUser` itself already uses,
/// so only the `401` case is folded into `Ok(None)` here; anything else
/// (today, only the `500` DB-error case) propagates as a real error
/// response, exactly like it would for a route requiring auth outright.
pub struct OptionalAuthenticatedUser(pub Option<AuthenticatedUser>);

impl FromRequestParts<App> for OptionalAuthenticatedUser {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        match AuthenticatedUser::from_request_parts(parts, app).await {
            Ok(user) => Ok(OptionalAuthenticatedUser(Some(user))),
            Err((StatusCode::UNAUTHORIZED, _)) => Ok(OptionalAuthenticatedUser(None)),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod sliding_session_tests {
    use axum::http::Request;
    use sqlx::PgPool;

    use super::*;

    async fn authenticate(app: &App, token: &str) -> bool {
        let request = Request::builder()
            .header(
                axum::http::header::COOKIE,
                format!("{SESSION_COOKIE_NAME}={token}"),
            )
            .body(axum::body::Body::empty())
            .expect("build request");
        let (mut parts, _body) = request.into_parts();
        AuthenticatedUser::from_request_parts(&mut parts, app)
            .await
            .is_ok()
    }

    async fn expires_in_hours(pool: &PgPool, token: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT ROUND(EXTRACT(EPOCH FROM expires_at - NOW()) / 3600)::bigint \
             FROM sessions WHERE id = $1",
        )
        .bind(hash_session_token(token))
        .fetch_one(pool)
        .await
        .expect("read expiry")
    }

    /// An authenticated request slides a due session to 14 days from now,
    /// and one inside the throttle leaves it where it is.
    #[sqlx::test(migrations = "../ds-store/migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn an_authenticated_request_slides_a_due_session(pool: PgPool) {
        let app = crate::test_support::inert_app(pool.clone());
        sqlx::query("INSERT INTO users (id) VALUES ('TEST-USER-SLIDE-EXTRACTOR')")
            .execute(&pool)
            .await
            .expect("insert user");
        for (token, expires_in) in [("due-token", "2 days"), ("fresh-token", "13 days 12 hours")] {
            sqlx::query(&format!(
                "INSERT INTO sessions (id, user_id, created_at, expires_at) \
                 VALUES ($1, 'TEST-USER-SLIDE-EXTRACTOR', NOW() - INTERVAL '12 days', \
                         NOW() + INTERVAL '{expires_in}')"
            ))
            .bind(hash_session_token(token))
            .execute(&pool)
            .await
            .expect("insert session");
        }

        assert!(authenticate(&app, "due-token").await);
        assert_eq!(expires_in_hours(&pool, "due-token").await, 14 * 24);
        assert!(authenticate(&app, "fresh-token").await);
        assert_eq!(expires_in_hours(&pool, "fresh-token").await, 13 * 24 + 12);
    }
}

#[cfg(test)]
mod internal_oauth_middleware_tests {
    use super::*;

    #[test]
    fn bearer_token_extracts_the_token_from_a_well_formed_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer abc.def.ghi".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), Some("abc.def.ghi".to_string()));
    }

    #[test]
    fn bearer_token_returns_none_without_the_bearer_prefix() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "abc.def.ghi".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn bearer_token_returns_none_with_no_authorization_header_at_all() {
        let headers = HeaderMap::new();
        assert_eq!(bearer_token(&headers), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cookie_finds_a_single_named_cookie() {
        let mut headers = HeaderMap::new();
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
        let mut headers = HeaderMap::new();
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
        let mut headers = HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "theme=dark".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "distant_signal_session"), None);
    }

    #[test]
    fn parse_cookie_returns_none_with_no_cookie_header_at_all() {
        let headers = HeaderMap::new();
        assert_eq!(parse_cookie(&headers, "distant_signal_session"), None);
    }

    #[test]
    fn set_cookie_header_includes_all_required_attributes_when_secure() {
        let header = set_cookie_header("distant_signal_session", "abc123", 7_776_000, true);
        assert!(header.starts_with("distant_signal_session=abc123;"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("Secure"));
        assert!(header.contains("SameSite=Lax"));
        assert!(header.contains("Max-Age=7776000"));
        assert!(header.contains("Path=/"));
    }

    #[test]
    fn set_cookie_header_omits_secure_over_plain_http() {
        // A `Secure` cookie is unconditionally rejected by the browser over
        // plain HTTP -- this is the live bug this parameter fixes.
        let header = set_cookie_header("distant_signal_session", "abc123", 7_776_000, false);
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

    #[test]
    fn expected_browser_origin_strips_path_and_keeps_scheme_and_host() {
        assert_eq!(
            expected_browser_origin("https://rail.example.com/api/auth/callback"),
            Some("https://rail.example.com".to_string())
        );
    }

    #[test]
    fn expected_browser_origin_keeps_a_non_default_port() {
        assert_eq!(
            expected_browser_origin("http://localhost:3000/api/auth/callback"),
            Some("http://localhost:3000".to_string())
        );
    }

    #[test]
    fn expected_browser_origin_returns_none_for_an_unparseable_url() {
        assert_eq!(expected_browser_origin("not-a-url"), None);
    }

    #[test]
    fn is_same_origin_accepts_a_matching_origin_header() {
        assert!(is_same_origin(
            Some("https://rail.example.com"),
            None,
            "https://rail.example.com"
        ));
    }

    #[test]
    fn is_same_origin_rejects_a_mismatched_origin_header() {
        assert!(!is_same_origin(
            Some("https://evil.example.com"),
            None,
            "https://rail.example.com"
        ));
    }

    #[test]
    fn is_same_origin_falls_back_to_referer_when_origin_is_absent() {
        assert!(is_same_origin(
            None,
            Some("https://rail.example.com/some/page?x=1"),
            "https://rail.example.com"
        ));
    }

    #[test]
    fn is_same_origin_rejects_a_mismatched_referer() {
        assert!(!is_same_origin(
            None,
            Some("https://evil.example.com/some/page"),
            "https://rail.example.com"
        ));
    }

    #[test]
    fn is_same_origin_rejects_when_neither_header_is_present() {
        // A real browser-submitted POST (or any genuine fetch/XHR) always
        // carries at least one of these -- refuse to guess rather than let
        // a request with neither through.
        assert!(!is_same_origin(None, None, "https://rail.example.com"));
    }

    #[test]
    fn is_same_origin_prefers_origin_over_a_mismatched_referer() {
        // `Origin`, when present, is authoritative -- a `Referer` fallback
        // is only ever consulted in its absence.
        assert!(is_same_origin(
            Some("https://rail.example.com"),
            Some("https://evil.example.com/page"),
            "https://rail.example.com"
        ));
    }
}

/// Coverage for `OptionalAuthenticatedUser`'s "no session" vs "DB error"
/// distinction -- the fix this suite exists to pin: before it,
/// `OptionalAuthenticatedUser`'s `Rejection` was `Infallible`, so a
/// transient Postgres error during the session lookup collapsed into the
/// exact same `Ok(None)` ("not logged in") a missing/expired session
/// cookie already produces. Neither test here needs a live, reachable
/// Postgres: the "no cookie" case never touches `app.database` at all
/// (short-circuits inside `AuthenticatedUser::from_request_parts` before
/// any query), and the "DB error" case deliberately points `app.database`
/// at an address nothing listens on, so the query attempt itself fails
/// fast with a real connection error -- exactly the failure mode this fix
/// must propagate rather than swallow.
#[cfg(test)]
mod optional_authenticated_user_tests {
    use axum::http::Request;
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    /// A lazily-parsed, never-eagerly-connected `PgPool` at an address
    /// nothing listens on (port 1): the DB-error test queries through it,
    /// so the connection attempt fails immediately rather than hanging.
    fn test_app() -> App {
        crate::test_support::inert_app(
            PgPoolOptions::new()
                .connect_lazy("postgres://user:password@127.0.0.1:1/placeholder")
                .expect("build placeholder lazy pg pool"),
        )
    }

    #[tokio::test]
    async fn no_session_cookie_resolves_to_anonymous_not_an_error() {
        let app = test_app();
        let request = Request::builder()
            .body(axum::body::Body::empty())
            .expect("build request");
        let (mut parts, _body) = request.into_parts();

        let OptionalAuthenticatedUser(user) =
            OptionalAuthenticatedUser::from_request_parts(&mut parts, &app)
                .await
                .expect("a missing session cookie must resolve to Ok(None), not an error");
        assert!(user.is_none());
    }

    #[tokio::test]
    async fn a_db_error_during_session_lookup_propagates_as_an_error_not_anonymous() {
        let app = test_app();
        let request = Request::builder()
            .header(
                axum::http::header::COOKIE,
                format!("{SESSION_COOKIE_NAME}=some-token-value"),
            )
            .body(axum::body::Body::empty())
            .expect("build request");
        let (mut parts, _body) = request.into_parts();

        // Not `.expect_err(...)`: that requires the `Ok` type to implement
        // `Debug`, and `OptionalAuthenticatedUser`/`AuthenticatedUser`
        // deliberately don't (matching this file's `LoginState`'s own
        // no-`Debug` posture on auth-adjacent types -- nothing here is as
        // sensitive as a login-state secret, but there's no upside to
        // adding a derive purely to satisfy a test assertion).
        match OptionalAuthenticatedUser::from_request_parts(&mut parts, &app).await {
            Ok(_) => panic!(
                "a DB error during session lookup must propagate as an error, not collapse \
                 into Ok(None)"
            ),
            // `test_app`'s pool cannot connect, so since 2026-10-06 this is
            // the dependency-unavailable 503 (`crate::unavailable`), still an
            // error rather than anonymous.
            Err(err) => assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE),
        }
    }
}

#[cfg(test)]
mod cross_origin_mutation_tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use axum::middleware;
    use axum::routing::post;
    use tower::ServiceExt;

    use super::{SESSION_COOKIE_NAME, reject_cross_origin_cookie_mutation};

    const SITE: &str = "https://ds.example.test";

    fn router(expected: Option<&str>) -> Router {
        Router::new()
            .route(
                "/groups/join/tok",
                post(|| async { StatusCode::NO_CONTENT }).get(|| async { StatusCode::OK }),
            )
            .layer(middleware::from_fn_with_state(
                expected.map(Arc::<str>::from),
                reject_cross_origin_cookie_mutation,
            ))
    }

    async fn status(
        expected: Option<&str>,
        method: Method,
        cookie: bool,
        origin: Option<&str>,
        referer: Option<&str>,
    ) -> StatusCode {
        let mut builder = Request::builder().method(method).uri("/groups/join/tok");
        if cookie {
            builder = builder.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}=abc"));
        }
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if let Some(referer) = referer {
            builder = builder.header(header::REFERER, referer);
        }
        router(expected)
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    /// The L4 attack: a no-body, cookie-bearing POST from a sibling origin.
    #[tokio::test]
    async fn a_cookie_bearing_post_from_another_origin_is_rejected() {
        let got = status(
            Some(SITE),
            Method::POST,
            true,
            Some("https://evil.example.test"),
            None,
        )
        .await;
        assert_eq!(got, StatusCode::FORBIDDEN);
        let got = status(
            Some(SITE),
            Method::DELETE,
            true,
            None,
            Some("https://evil.example.test/x"),
        )
        .await;
        assert_eq!(got, StatusCode::FORBIDDEN);
        let got = status(Some(SITE), Method::POST, true, Some("null"), None).await;
        assert_eq!(got, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_same_origin_cookie_bearing_post_passes() {
        let got = status(Some(SITE), Method::POST, true, Some(SITE), None).await;
        assert_eq!(got, StatusCode::NO_CONTENT);
        let got = status(
            Some(SITE),
            Method::POST,
            true,
            None,
            Some("https://ds.example.test/groups/1"),
        )
        .await;
        assert_eq!(got, StatusCode::NO_CONTENT);
    }

    /// Server-side fetches (frontend `lib/api.ts`) and non-browser clients
    /// send no Origin/Referer at all; a request with no session cookie has
    /// nothing ambient to abuse; safe methods are never state-changing.
    #[tokio::test]
    async fn headerless_cookieless_safe_and_unconfigured_requests_pass() {
        assert_eq!(
            status(Some(SITE), Method::POST, true, None, None).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            status(
                Some(SITE),
                Method::POST,
                false,
                Some("https://evil.example.test"),
                None
            )
            .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            status(
                Some(SITE),
                Method::GET,
                true,
                Some("https://evil.example.test"),
                None
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            status(
                None,
                Method::POST,
                true,
                Some("https://evil.example.test"),
                None
            )
            .await,
            StatusCode::NO_CONTENT
        );
    }
}
