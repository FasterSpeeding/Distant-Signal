//! Internal-auth gate for `private_router()`.
//!
//! Bearer-token OAuth2 client-credentials auth (RFC 6750/6749 §4.4),
//! delegated to Authentik. `require_internal_oauth` parses the
//! `Authorization: Bearer` header, verifies it against Authentik's JWKS
//! (`internal_oauth::ServiceTokenVerifier`, local, no per-request network
//! round trip once cached), then matches the request's path AND method
//! against a static, config-built route-scoping table
//! (`App::internal_oauth_routes`), and finally checks the verified
//! token's `groups` claim against that matched entry's required group
//! names -- a route passes if `groups` contains ANY of them. The method
//! dimension is load-bearing, not incidental: `/stanox-crs` has two
//! legitimate callers, `trust-consumer` (read-only, `GET` only) and
//! `schedule-reference` (write-only, `POST` only), each with its OWN
//! table entry carrying only its own group -- a token good for one
//! method on a path is never treated as good for a different method on
//! that same path just because some group would otherwise be allowed
//! there. See
//! docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md.
//! This replaces a single shared-secret header, compared in fixed time
//! against one configured string with no concept of *which* caller
//! presented it -- that scheme is retired outright, not kept alongside
//! this one (no dual-acceptance window).

pub mod internal_oauth;
pub mod oidc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderMap, StatusCode, request::Parts};
use axum::middleware::Next;
use axum::response::Response;

use crate::app::App;

/// `axum::middleware::from_fn_with_state` handler enforcing internal-service
/// OAuth2 auth. Applied only to `private_router()` -- `public_router()`
/// never sees this.
///
/// Status codes: a missing/malformed/expired/signature-invalid/wrong-
/// issuer/wrong-audience bearer token -> `401`, collapsed into one outcome
/// deliberately (see `VerifyError`'s own doc comment) -- a caller
/// presenting a token that fails verification for any of these reasons
/// learns only "not accepted," never which specific check failed. A path
/// absent from the scoping table entirely, OR present but not for the
/// request's method, OR present for that exact (path, method) but whose
/// required group(s) the verified token's `groups` claim doesn't contain
/// -> `403` in all three cases, with the token's `sub` and the request
/// path (and, for the latter two, method) logged -- a real,
/// Authentik-issued credential, just not scoped for this route, which is
/// actionable signal for a misconfigured deployment (a chart/secret
/// wiring mistake handing one service another's credential), not an
/// information leak (the route table itself is fixed and not secret).
pub async fn require_internal_oauth(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    let Some(token) = bearer_token(request.headers()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let claims = match app.internal_oauth_verifier.verify(&token).await {
        Ok(claims) => claims,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Two-phase lookup, deliberately not a single `.find()` keyed on
    // (path, method) together: a path known to the table but hit with
    // the wrong method needs to be distinguishable (for logging/clarity)
    // from a path the table has never heard of at all. Phase 1 matches
    // path only.
    let path_matches: Vec<_> = app
        .internal_oauth_routes
        .iter()
        .filter(|(prefix, _, _)| path.starts_with(prefix))
        .collect();

    if path_matches.is_empty() {
        // No entry for this path at all -- default-deny even for a
        // perfectly valid token, rather than silently "allowed" because
        // nobody added its row.
        tracing::warn!(sub = %claims.sub, path, "internal oauth request rejected: no route-scoping entry for this path");
        return Err(StatusCode::FORBIDDEN);
    }

    // Phase 2: among the path-matching entries, require one whose method
    // also matches. A route known to the table under a DIFFERENT method
    // (e.g. `/stanox-crs` has a `GET` entry and a separate `POST` entry)
    // must never fall back to some other method's required group --
    // that's exactly the gap this method dimension exists to close.
    let Some((_, _, required_groups)) = path_matches
        .into_iter()
        .find(|(_, entry_method, _)| *entry_method == method)
    else {
        tracing::warn!(sub = %claims.sub, path, %method, "internal oauth request rejected: valid token, wrong method for this route");
        return Err(StatusCode::FORBIDDEN);
    };

    if !required_groups
        .iter()
        .any(|group| claims.groups.contains(group))
    {
        tracing::warn!(sub = %claims.sub, path, %method, "internal oauth request rejected: valid token, wrong scope");
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
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
    pub groups: Vec<String>,
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
            groups: session.groups,
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

/// Wraps `AuthenticatedUser` with one more lookup against
/// `chatbot_allowed_users` -- the DS-hosted chat orchestrator (Option B)'s
/// cost/access gate, embedded-chatbot-dual-mode-design's Decision 5. See
/// `docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md` Task 2.
///
/// Deliberately a SEPARATE rejection shape from `AuthenticatedUser`'s own
/// `401` ("no session at all"): a resolved, real user who simply isn't on
/// the list is a genuinely different case and, per that design's own Error
/// handling section, must not collapse into a `404` -- this isn't an
/// ownership check hiding a secret resource, the feature's existence isn't
/// a secret, so a logged-in-but-not-allowlisted user gets a plain `403`
/// "not available for your account" instead.
pub struct ChatbotAuthorizedUser(pub AuthenticatedUser);

impl FromRequestParts<App> for ChatbotAuthorizedUser {
    type Rejection = (axum::http::StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, app)
            .await
            .map_err(|(status, msg)| (status, axum::Json(serde_json::json!({ "error": msg }))))?;
        let allowed = crate::data::users::is_chatbot_allowed(&app.database, &user.id)
            .await
            .map_err(|err| {
                tracing::error!(error = ?err, "chatbot allowlist lookup failed");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({ "error": "chatbot allowlist lookup failed" })),
                )
            })?;
        if !allowed {
            return Err((
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({ "error": "chatbot_not_available" })),
            ));
        }
        Ok(ChatbotAuthorizedUser(user))
    }
}

#[cfg(test)]
mod internal_oauth_middleware_tests {
    use super::*;

    #[test]
    fn bearer_token_extracts_the_token_from_a_well_formed_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer abc.def.ghi".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), Some("abc.def.ghi".to_string()));
    }

    #[test]
    fn bearer_token_returns_none_without_the_bearer_prefix() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "abc.def.ghi".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn bearer_token_returns_none_with_no_authorization_header_at_all() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(bearer_token(&headers), None);
    }

    // The route-scoping lookup itself (path match, then method match,
    // then `groups.contains`) is exercised end-to-end, not here --
    // see `route_scoping_tests` below, which builds a real `AppState`
    // (via `crate::app::build_internal_oauth_routes`, the actual
    // production table, not a hand-copied stand-in) and drives real
    // `Request`s with real, wiremock-signed JWTs through
    // `require_internal_oauth` itself.
}

#[cfg(test)]
mod tests {
    use super::*;

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

/// End-to-end coverage for `require_internal_oauth`'s route-scoping check
/// -- the security gap this test suite exists to close (and pin against
/// regressing) is that a route-scoping table keyed on path alone lets
/// EITHER of `/stanox-crs`'s two legitimate callers authorize BOTH `GET`
/// and `POST` on it, when only one method is actually legitimate per
/// caller. Builds a real `App` (real `AppState`, a real mocked-Authentik
/// `ServiceTokenVerifier`, and -- critically -- the REAL production
/// route table via `crate::app::build_internal_oauth_routes`, not a
/// hand-copied stand-in that could silently drift from it) and drives
/// real `axum::http::Request`s, with real signed-and-verified JWTs, all
/// the way through `require_internal_oauth` via `tower::ServiceExt::
/// oneshot` -- mirrors `routes::lines::db_tests`'s established
/// `test_router`/`.oneshot(..)` pattern, minus that module's Postgres
/// dependency (this middleware never touches the database, so `database`
/// here is a lazily-parsed, never-connected pool -- see `test_app`'s own
/// doc comment).
#[cfg(test)]
mod route_scoping_tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use axum::middleware;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;
    use wiremock::MockServer;

    use super::require_internal_oauth;
    use crate::app::{App, AppState, build_internal_oauth_routes};
    use crate::auth::internal_oauth::test_support::{mock_authentik, sign_token, valid_claims};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Every caller gets its OWN group name, matching real deployment
    /// (Decision 3: one group per real caller, never shared) -- so a
    /// wrong-caller's token failing a check here is unambiguously "wrong
    /// caller," never a coincidental group-name collision this fixture
    /// introduced by accident.
    fn test_config() -> ServiceArguments {
        ServiceArguments {
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
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(Vec::new()),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
        }
    }

    /// Returns `(mock Authentik server, the App under test, the
    /// production route table `App` was built with)`. The `MockServer`
    /// MUST stay alive (bound, not `_`-discarded) for as long as the
    /// returned `App`/router is used -- `ServiceTokenVerifier` fetches
    /// its JWKS lazily, on first `verify()` call, not at construction, so
    /// a dropped-and-shut-down mock server would only surface as a
    /// confusing failure on the FIRST request a test sends, not here.
    ///
    /// `database` is a lazily-parsed `PgPool` (`connect_lazy` -- parses
    /// the URL, never opens a socket, same "no eager connect" posture as
    /// `AppState::redis`'s own doc comment) rather than a real connected
    /// pool or `#[sqlx::test]`: `require_internal_oauth` never touches
    /// `app.database` at all, so this suite has no business requiring a
    /// live Postgres the way `routes::lines::db_tests` does.
    async fn test_app() -> (MockServer, App, Vec<(&'static str, Method, Vec<String>)>) {
        let (server, verifier) = mock_authentik().await;
        let config = test_config();
        let internal_oauth_routes = build_internal_oauth_routes(&config);
        let expected_routes = internal_oauth_routes.clone();

        let app = std::sync::Arc::new(AppState {
            config,
            database: PgPoolOptions::new()
                .connect_lazy("postgres://user:password@127.0.0.1:0/placeholder")
                .expect("build placeholder lazy pg pool"),
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: verifier,
            internal_oauth_routes,
        });

        (server, app, expected_routes)
    }

    /// A minimal router carrying ONLY `require_internal_oauth` -- no real
    /// `ingest`/`samples` handlers (which would need a live database) --
    /// so a request that gets past the middleware always hits a fixed
    /// `200 OK` fallback. What matters for this suite is entirely what
    /// the middleware itself does with the request BEFORE that point.
    fn test_router(app: App) -> axum::Router {
        async fn ok() -> StatusCode {
            StatusCode::OK
        }

        crate::app::Router::new()
            .fallback(ok)
            .layer(middleware::from_fn_with_state(
                app.clone(),
                require_internal_oauth,
            ))
            .with_state(app)
    }

    fn token_for(issuer: &str, sub: &str, groups: &[&str]) -> String {
        let groups: Vec<String> = groups.iter().map(|g| g.to_string()).collect();
        sign_token(&valid_claims(issuer, |c| {
            c["sub"] = json!(sub);
            c["groups"] = json!(groups);
        }))
    }

    async fn send(
        router: &axum::Router,
        method: Method,
        path: &str,
        token: Option<&str>,
    ) -> StatusCode {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = builder.body(Body::empty()).expect("build request");
        router
            .clone()
            .oneshot(request)
            .await
            .expect("oneshot request")
            .status()
    }

    #[tokio::test]
    async fn trust_consumers_token_is_accepted_on_get_stanox_crs() {
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-trust-consumer-1",
            &["svc-trust-consumer"],
        );

        assert_eq!(
            send(&router, Method::GET, "/stanox-crs", Some(&token)).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn trust_consumers_token_is_rejected_on_post_stanox_crs() {
        // The bug this whole suite exists to pin: before the fix, a
        // route-scoping table keyed on path alone let trust-consumer's
        // READ-ONLY token authorize a WRITE to the STANOX/CRS reference
        // table it should only ever read.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-trust-consumer-1",
            &["svc-trust-consumer"],
        );

        assert_eq!(
            send(&router, Method::POST, "/stanox-crs", Some(&token)).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn schedule_references_token_is_accepted_on_post_stanox_crs() {
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-schedule-reference-1",
            &["svc-schedule-reference"],
        );

        assert_eq!(
            send(&router, Method::POST, "/stanox-crs", Some(&token)).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn schedule_references_token_is_rejected_on_get_stanox_crs() {
        // The other half of the same bug: schedule-reference's WRITE-ONLY
        // token must not also authorize reading the table back out.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-schedule-reference-1",
            &["svc-schedule-reference"],
        );

        assert_eq!(
            send(&router, Method::GET, "/stanox-crs", Some(&token)).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn a_token_with_no_matching_group_at_all_is_rejected_on_stanox_crs() {
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-poller-incidents-1",
            &["svc-poller-incidents"],
        );

        assert_eq!(
            send(&router, Method::GET, "/stanox-crs", Some(&token)).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(&router, Method::POST, "/stanox-crs", Some(&token)).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn a_request_with_no_bearer_token_is_rejected_unauthorized() {
        let (_server, app, _routes) = test_app().await;
        let router = test_router(app.clone());

        assert_eq!(
            send(&router, Method::GET, "/stanox-crs", None).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn a_path_absent_from_the_table_entirely_is_rejected_forbidden_even_with_a_valid_token() {
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-trust-consumer-1",
            &["svc-trust-consumer"],
        );

        assert_eq!(
            send(&router, Method::GET, "/some-unknown-route", Some(&token)).await,
            StatusCode::FORBIDDEN
        );
    }

    /// Regression coverage for every OTHER route the fix touched: for
    /// each `(path, method, groups)` entry in the REAL production table
    /// (`build_internal_oauth_routes`, not a hand-copied list here), a
    /// token carrying that entry's own required group must still be
    /// accepted on that exact method. Every entry in the fixed table
    /// carries exactly one group (the `/stanox-crs` split was the only
    /// multi-group entry that ever existed), so this loop can mint one
    /// token per entry without needing per-route special-casing. This is
    /// the check that "nothing else broke" -- if a future edit to
    /// `build_internal_oauth_routes` drops or mis-scopes any entry, this
    /// test fails alongside the two explicit `/stanox-crs` tests above.
    #[tokio::test]
    async fn every_production_route_entry_is_reachable_by_its_own_caller_on_its_own_method() {
        let (server, app, routes) = test_app().await;
        let router = test_router(app.clone());

        for (path, method, groups) in &routes {
            assert_eq!(
                groups.len(),
                1,
                "expected exactly one group per route entry post-fix: {path} {method}"
            );
            let token = token_for(&server.uri(), "svc-under-test", &[groups[0].as_str()]);

            let status = send(&router, method.clone(), path, Some(&token)).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "expected {method} {path} to accept its own caller's token"
            );
        }
    }
}
