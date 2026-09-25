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
//! dimension is load-bearing, not incidental: `/stanox-crs` has four
//! legitimate callers -- `trust-consumer`, `full-coverage-consumer`, and
//! `trust-backlog-consumer` (read-only, `GET` only, sharing one table
//! entry) and `schedule-reference` (write-only, `POST` only, its own
//! entry) -- a token good for one
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
        .filter(|(prefix, _, _)| path_matches_route(&path, prefix))
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

/// Segment-aware "does `path` fall under this table entry's `prefix`"
/// check -- deliberately NOT a plain `path.starts_with(prefix)`. Every
/// entry in `App::internal_oauth_routes` today happens to be registered as
/// an exact route (no sibling path is ever a literal string-prefix of
/// another, e.g. nothing named `/stanox-crs-extra` exists alongside
/// `/stanox-crs`), so a bare `starts_with` has never actually
/// misattributed a request -- but that's a property of today's route list,
/// not something this check enforced. A plain prefix match would let a
/// future route added as (say) `/stanox` silently authorize requests to
/// `/stanox-crs` too (or vice versa), with no compiler error and no test
/// failure unless someone thought to add one for that exact pair -- purely
/// latent risk with zero CI signal to catch it. Requiring the byte right
/// after `prefix` to be either the end of the string or a `/` closes that
/// gap: a match only counts at a real path-segment boundary, exactly like
/// this table's own individual entries are written (`/stanox-crs`, not
/// `/stanox-crs*`).
fn path_matches_route(path: &str, prefix: &str) -> bool {
    match path.strip_prefix(prefix) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
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
/// mirroring `frontend/app/connect-claude/authorize/route.ts`'s own
/// `isSameOriginRequest` -- see that function's doc comment for the fuller
/// rationale this codebase already committed to there: `SameSite=Lax` is
/// this app's only other CSRF precedent, and it's enforced entirely
/// client-side (by the browser) with nothing backing it up server-side.
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
/// arrives there via a cross-origin redirect FROM the IdP, so `Referer`
/// legitimately names the IdP's own origin, never this app's).
pub fn is_same_origin(origin: Option<&str>, referer: Option<&str>, expected_origin: &str) -> bool {
    if let Some(origin) = origin {
        return origin == expected_origin;
    }
    if let Some(referer) = referer {
        return openidconnect::url::Url::parse(referer)
            .map(|url| url.origin().ascii_serialization() == expected_origin)
            .unwrap_or(false);
    }
    false
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
    type Rejection = (axum::http::StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        match AuthenticatedUser::from_request_parts(parts, app).await {
            Ok(user) => Ok(OptionalAuthenticatedUser(Some(user))),
            Err((axum::http::StatusCode::UNAUTHORIZED, _)) => Ok(OptionalAuthenticatedUser(None)),
            Err(err) => Err(err),
        }
    }
}

/// Wraps `AuthenticatedUser` with one more check: does the resolved user's
/// own `groups` (the already-decoded OIDC `groups` claim, upserted onto
/// `users.groups` on every login -- see `data::users::upsert_user`) contain
/// the configured `ServiceArguments::chatbot_access_group`? This is the
/// DS-hosted embedded chatbot's access gate, embedded-chatbot-dual-mode-
/// design's Decision 5. See
/// `docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md` Task 2.
///
/// Formerly backed by a per-user `chatbot_allowed_users` DB allowlist
/// (`data::users::is_chatbot_allowed`, an extra async DB round trip); now a
/// synchronous check against the `groups` `AuthenticatedUser::
/// from_request_parts` already resolved for the `401` check above -- an SSO
/// group is a strictly better fit for "which real people get this" than a
/// hand-maintained per-user table: an operator manages membership in
/// Authentik, the same place every other access group in this app already
/// lives, instead of a bespoke admin path onto this one table. The
/// allowlist table itself is dropped (see the migration removing it);
/// nothing else read it.
///
/// Deliberately a SEPARATE rejection shape from `AuthenticatedUser`'s own
/// `401` ("no session at all"): a resolved, real user who simply isn't in
/// the group is a genuinely different case and, per that design's own Error
/// handling section, must not collapse into a `404` -- this isn't an
/// ownership check hiding a secret resource, the feature's existence isn't
/// a secret, so a logged-in-but-not-in-group user gets a plain `403`
/// "not available for your account" instead.
pub struct ChatbotAuthorizedUser(pub AuthenticatedUser);

/// Does `groups` (a resolved user's own group memberships) include the
/// configured chatbot-access SSO group? Split out from the extractor below
/// so it's testable without a live database -- `AuthenticatedUser::
/// from_request_parts` (the only path that produces a real `groups` list)
/// always hits `sessions`/`users`, but this check itself needs neither
/// those tables nor the removed `chatbot_allowed_users` one.
fn has_chatbot_access(groups: &[String], required_group: &str) -> bool {
    groups.iter().any(|group| group == required_group)
}

impl FromRequestParts<App> for ChatbotAuthorizedUser {
    type Rejection = (axum::http::StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, app)
            .await
            .map_err(|(status, msg)| (status, axum::Json(serde_json::json!({ "error": msg }))))?;
        if !has_chatbot_access(&user.groups, &app.config.chatbot_access_group) {
            return Err((
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({ "error": "chatbot_not_available" })),
            ));
        }
        Ok(ChatbotAuthorizedUser(user))
    }
}

#[cfg(test)]
mod chatbot_access_tests {
    use super::*;

    #[test]
    fn a_user_whose_groups_contains_the_configured_group_is_granted_access() {
        let groups = vec![
            "mcp-users".to_string(),
            "distant-signal-chatbot-users".to_string(),
        ];
        assert!(has_chatbot_access(&groups, "distant-signal-chatbot-users"));
    }

    #[test]
    fn a_user_whose_groups_does_not_contain_the_configured_group_is_denied_access() {
        let groups = vec!["mcp-users".to_string()];
        assert!(!has_chatbot_access(&groups, "distant-signal-chatbot-users"));
    }

    #[test]
    fn a_user_with_no_groups_at_all_is_denied_access() {
        let groups: Vec<String> = Vec::new();
        assert!(!has_chatbot_access(&groups, "distant-signal-chatbot-users"));
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

    #[test]
    fn path_matches_route_accepts_an_exact_match() {
        assert!(path_matches_route("/stanox-crs", "/stanox-crs"));
    }

    #[test]
    fn path_matches_route_accepts_a_real_sub_path_at_a_segment_boundary() {
        assert!(path_matches_route("/stanox-crs/123", "/stanox-crs"));
    }

    #[test]
    fn path_matches_route_rejects_a_sibling_path_that_merely_shares_a_string_prefix() {
        // The regression this check exists to close: `/private/foo` must
        // never be treated as a prefix-match for a request actually bound
        // for `/private/foobar` (or vice versa) just because one string
        // happens to start with the other -- only a match at a real `/`
        // segment boundary counts.
        assert!(!path_matches_route("/private/foobar", "/private/foo"));
        assert!(!path_matches_route("/private/foo", "/private/foobar"));
    }

    #[test]
    fn path_matches_route_rejects_an_unrelated_path() {
        assert!(!path_matches_route("/stations", "/stanox-crs"));
    }
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

/// End-to-end coverage for `require_internal_oauth`'s route-scoping check
/// -- the security gap this test suite exists to close (and pin against
/// regressing) is that a route-scoping table keyed on path alone lets
/// ANY of `/stanox-crs`'s four legitimate callers -- `trust-consumer`,
/// `full-coverage-consumer`, and `trust-backlog-consumer` (`GET`) and
/// `schedule-reference` (`POST`) -- authorize BOTH `GET`
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
    pub(super) fn test_config() -> ServiceArguments {
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
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
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
            lines: LineCatalogue(Vec::new()),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
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
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
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
            schedule_crs_line_index: std::collections::HashMap::new(),
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
    async fn full_coverage_consumers_token_is_accepted_on_get_stanox_crs() {
        // full-coverage-consumer is the second legitimate GET caller on
        // this path (alongside trust-consumer) -- confirmed live 2026-09-05:
        // its otherwise-correctly-scoped token 403'd here because this
        // route's group list never included it, despite its config.rs
        // carrying a stanox_crs_url since Deploy A.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-full-coverage-consumer-1",
            &["svc-full-coverage-consumer"],
        );

        assert_eq!(
            send(&router, Method::GET, "/stanox-crs", Some(&token)).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn trust_backlog_consumers_token_is_accepted_on_get_stanox_crs() {
        // trust-backlog-consumer is the third legitimate GET caller on this
        // path (alongside trust-consumer and full-coverage-consumer): its
        // hourly STANOX/CRS reference-table reload otherwise-correctly-scoped
        // token 403'd here because this route's group list never included
        // it.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-trust-backlog-consumer-1",
            &["svc-trust-backlog-consumer"],
        );

        assert_eq!(
            send(&router, Method::GET, "/stanox-crs", Some(&token)).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn trust_backlog_consumers_token_is_rejected_on_post_stanox_crs() {
        // Same read-only boundary as trust-consumer's and
        // full-coverage-consumer's own tokens: being an accepted GET caller
        // must not also authorize the write.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-trust-backlog-consumer-1",
            &["svc-trust-backlog-consumer"],
        );

        assert_eq!(
            send(&router, Method::POST, "/stanox-crs", Some(&token)).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn full_coverage_consumers_token_is_rejected_on_post_stanox_crs() {
        // Same read-only boundary as trust-consumer's own token: being an
        // accepted GET caller must not also authorize the write.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-full-coverage-consumer-1",
            &["svc-full-coverage-consumer"],
        );

        assert_eq!(
            send(&router, Method::POST, "/stanox-crs", Some(&token)).await,
            StatusCode::FORBIDDEN
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
    async fn schedule_references_token_is_accepted_on_post_tiploc_crs() {
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let token = token_for(
            &server.uri(),
            "svc-schedule-reference-1",
            &["svc-schedule-reference"],
        );

        assert_eq!(
            send(&router, Method::POST, "/tiploc-crs", Some(&token)).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn every_other_services_token_is_rejected_on_post_tiploc_crs() {
        // /tiploc-crs is POST-only with exactly one legitimate caller
        // (schedule-reference, reusing its existing writer credential --
        // see app.rs's route-scoping table). Every OTHER service's own
        // token, each individually valid for its own routes, must still be
        // rejected here -- mirrors /stanox-crs POST's own
        // trust_consumers_token_is_rejected_on_post_stanox_crs /
        // full_coverage_consumers_token_is_rejected_on_post_stanox_crs /
        // trust_backlog_consumers_token_is_rejected_on_post_stanox_crs
        // tests above, generalized to every other caller group at once
        // since there is no GET pair here splitting the boundary in two.
        let (server, app, _routes) = test_app().await;
        let router = test_router(app.clone());
        let config = test_config();
        let other_groups = [
            &config.internal_oauth_group_incidents,
            &config.internal_oauth_group_stations,
            &config.internal_oauth_group_tocs,
            &config.internal_oauth_group_ldbws,
            &config.internal_oauth_group_tfl,
            &config.internal_oauth_group_trust_consumer,
            &config.internal_oauth_group_schedule_ingest,
            &config.internal_oauth_group_full_coverage,
            &config.internal_oauth_group_trust_backlog,
            &config.internal_oauth_group_irish_rail_gtfs,
            &config.internal_oauth_group_irish_rail_live,
            &config.internal_oauth_group_nir_stations,
        ];

        for group in other_groups {
            let token = token_for(&server.uri(), "svc-under-test", &[group.as_str()]);
            assert_eq!(
                send(&router, Method::POST, "/tiploc-crs", Some(&token)).await,
                StatusCode::FORBIDDEN,
                "expected group {group} to be rejected on POST /tiploc-crs"
            );
        }
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
    /// token carrying ANY ONE of that entry's own required groups must
    /// still be accepted on that exact method -- tested individually per
    /// group (not just the union), since the whole point of a caller
    /// being listed is that ITS OWN token, alone, is sufficient. A couple
    /// of entries now carry more than one group -- GET /stanox-crs
    /// (trust-consumer, full-coverage-consumer, and trust-backlog-consumer
    /// all read it) and GET /schedule-feed-ingests (schedule-ingest and
    /// schedule-reference both read it) -- every other entry still carries
    /// exactly one. This is the check that "nothing else broke" -- if a
    /// future edit to
    /// `build_internal_oauth_routes` drops or mis-scopes any entry, this
    /// test fails alongside the explicit `/stanox-crs` tests above.
    #[tokio::test]
    async fn every_production_route_entry_is_reachable_by_each_of_its_own_callers_on_its_own_method()
     {
        let (server, app, routes) = test_app().await;
        let router = test_router(app.clone());

        for (path, method, groups) in &routes {
            assert!(
                !groups.is_empty(),
                "route entry has no required groups at all: {path} {method}"
            );
            for group in groups {
                let token = token_for(&server.uri(), "svc-under-test", &[group.as_str()]);

                let status = send(&router, method.clone(), path, Some(&token)).await;
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "expected {method} {path} to accept caller group {group}'s token"
                );
            }
        }
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

    use super::route_scoping_tests::test_config;
    use super::*;
    use crate::app::AppState;
    use crate::auth::oidc::{OidcClient, OidcConfig};

    /// Mirrors `route_scoping_tests::test_app`'s "lazily-parsed, never-
    /// eagerly-connected `PgPool`" posture, with one deliberate
    /// difference: that suite never actually queries its placeholder pool
    /// (`require_internal_oauth` never touches `app.database`), so its
    /// placeholder address is never dialed. This suite's DB-error test
    /// DOES query through this pool -- that's the point -- so the address
    /// below (port 1, nothing ever listens there) is chosen to fail the
    /// connection attempt immediately rather than hang.
    fn test_app() -> App {
        let config = test_config();
        std::sync::Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            database: PgPoolOptions::new()
                .connect_lazy("postgres://user:password@127.0.0.1:1/placeholder")
                .expect("build placeholder lazy pg pool"),
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
            .expect("construct placeholder internal oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
            config,
        })
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
            Err(err) => assert_eq!(err.0, axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}
