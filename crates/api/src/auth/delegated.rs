//! Delegated access: a registered client (today only ds-mcp) acting for a
//! user who granted it access on the Connected apps page.
//! docs/superpowers/specs/2026-10-14-ds-mcp-delegated-access-design.md
//! §5.2-§5.11; the operator's view is docs/connected-apps.md.
//!
//! The pieces, in request order:
//!
//! 1. **The listener** (`main.rs`). Delegated bearers are accepted only on
//!    the delegated listener (`DELEGATED_LISTEN_ADDR`), an in-cluster port
//!    published by its own `ClusterIP` Service and admitted by the
//!    NetworkPolicy only from ds-mcp's pods (decision D-14). That listener
//!    tags every request it accepts with [`Ingress::Delegated`]; no client
//!    can set a request extension, so the public listener (tunnel,
//!    ingress, frontend proxy) can never pass for it.
//! 2. **[`gate`]**, a router-wide middleware ahead of the rate limiter. A
//!    bearer whose unverified `iss` and `aud` name a registered client is
//!    routed to that client's verifier (signature through the shared
//!    JWKS machinery, `iss`, `aud`, `exp`, `nbf`, a required `iat`, not a
//!    logout token). It is refused on the public listener, for a disabled
//!    client, with a session cookie alongside (400
//!    `ambiguous_credentials`), and on any route outside
//!    [`DELEGATED_ROUTES`]. Every other bearer is left to the rate
//!    limiter's internal-token path, unchanged.
//! 3. **The rate limiter** charges a verified delegated request to the
//!    user's and the client's delegated buckets (`crate::rate_limit`).
//! 4. **[`DelegatedOrSessionUser`]**, the extractor of the four opted-in
//!    handlers: the grant check (active, not idle 90 days, token issued
//!    after the grant's `not_before` and the user's
//!    `sessions_invalidated_at`) and the route's scope, intersected from
//!    the token, the grant and the client's `maxScopes`. With no delegated
//!    token it is exactly `AuthenticatedUser`, which this module never
//!    changes.
//!
//! There is no `act` claim to check (the 2026-10-14 spike, S3): the client
//! is identified by `iss` and `aud` alone. An empty `scope` still yields a
//! token (S4), so every route asks for its own scope.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, bail, ensure};
use axum::extract::{FromRequestParts, MatchedPath, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header, request::Parts};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::App;
use crate::auth::internal_oauth::{Audience, CLOCK_SKEW_LEEWAY, ServiceTokenVerifier, VerifyError};
use crate::auth::{AuthenticatedUser, SESSION_COOKIE_NAME, bearer_token, parse_cookie};

/// A scope a delegated client may hold (§5.5). `journeys:read_shared` is
/// Phase 3 and not accepted yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Scope {
    #[serde(rename = "journeys:read")]
    JourneysRead,
    #[serde(rename = "journeys:write")]
    JourneysWrite,
    #[serde(rename = "journeys:delete")]
    JourneysDelete,
}

impl Scope {
    pub const ALL: [Scope; 3] = [
        Scope::JourneysRead,
        Scope::JourneysWrite,
        Scope::JourneysDelete,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::JourneysRead => "journeys:read",
            Scope::JourneysWrite => "journeys:write",
            Scope::JourneysDelete => "journeys:delete",
        }
    }

    pub fn parse(value: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|scope| scope.as_str() == value)
    }
}

/// The routes a delegated caller may reach, with the scope each needs
/// (§5.8). [`gate`] refuses a delegated token on every other route before
/// any handler runs, and `routes::journeys`' tests pin this list against the
/// handlers that take [`DelegatedOrSessionUser`].
pub const DELEGATED_ROUTES: &[(Method, &str, Scope)] = &[
    (Method::GET, "/Journeys/mine", Scope::JourneysRead),
    (Method::GET, "/Journeys/{journey_id}", Scope::JourneysRead),
    (Method::POST, "/Journeys/itinerary", Scope::JourneysWrite),
    (
        Method::DELETE,
        "/Journeys/{journey_id}",
        Scope::JourneysDelete,
    ),
];

/// The scope [`DELEGATED_ROUTES`] gives a request, or `None` for a route a
/// delegated caller may not reach.
pub fn route_scope(method: &Method, matched_path: &str) -> Option<Scope> {
    DELEGATED_ROUTES
        .iter()
        .find(|(m, path, _)| m == method && *path == matched_path)
        .map(|(_, _, scope)| *scope)
}

/// Which listener a request arrived on. Only the delegated listener inserts
/// it (`main.rs`); a request without it came through the public listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ingress {
    Delegated,
}

/// The delegated listener's copy of the api's router (`main.rs`): the same
/// routes, with every request marked [`Ingress::Delegated`]. That mark is
/// the only thing that lets [`gate`] accept a delegated bearer, and only
/// this listener sets it.
pub fn delegated_listener_router(router: axum::Router) -> axum::Router {
    router.layer(axum::Extension(Ingress::Delegated))
}

/// How long a grant may go unused before it expires (D-6).
pub const GRANT_IDLE_DAYS: i64 = 90;

/// One `DELEGATED_CLIENTS` entry, as configured (camelCase JSON, the
/// chart's `api.delegated.clients` list).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawClient {
    name: String,
    display_name: String,
    issuer_url: String,
    audience: String,
    max_scopes: Vec<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    service_group: String,
}

fn default_true() -> bool {
    true
}

/// A registered client, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// The grant table's `client` value and `journeys.created_via`.
    pub name: String,
    /// Shown to users ("Distant Signal assistant").
    pub display_name: String,
    pub issuer_url: String,
    pub audience: String,
    pub max_scopes: Vec<Scope>,
    /// The kill switch: `false` refuses every token for this client (401).
    pub enabled: bool,
    /// The internal-OAuth group whose service token may revoke this
    /// client's grants (`POST /delegated-grants/revoke`). Empty: nobody.
    pub service_group: String,
}

/// Parses and validates `DELEGATED_CLIENTS`.
pub fn parse_clients(json: &str) -> Result<Vec<ClientConfig>> {
    let json = json.trim();
    if json.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawClient> =
        serde_json::from_str(json).context("DELEGATED_CLIENTS is not a JSON list of clients")?;
    let mut names = HashSet::new();
    let mut clients = Vec::with_capacity(raw.len());
    for client in raw {
        ensure!(
            valid_client_name(&client.name),
            "DELEGATED_CLIENTS: client name {:?} must be 1 to 32 of [a-z0-9-]",
            client.name
        );
        ensure!(
            client.name != "web",
            "DELEGATED_CLIENTS: `web` is reserved (journeys.created_via for the web app)"
        );
        ensure!(
            names.insert(client.name.clone()),
            "DELEGATED_CLIENTS: client name {:?} is listed twice",
            client.name
        );
        let display_name = client.display_name.trim().to_string();
        ensure!(
            (1..=100).contains(&display_name.chars().count()),
            "DELEGATED_CLIENTS: {}: displayName must be 1 to 100 characters",
            client.name
        );
        openidconnect::IssuerUrl::new(client.issuer_url.clone())
            .with_context(|| format!("DELEGATED_CLIENTS: {}: invalid issuerUrl", client.name))?;
        ensure!(
            !client.audience.trim().is_empty(),
            "DELEGATED_CLIENTS: {}: audience must not be empty",
            client.name
        );
        let mut max_scopes = Vec::new();
        for scope in &client.max_scopes {
            let Some(parsed) = Scope::parse(scope) else {
                bail!(
                    "DELEGATED_CLIENTS: {}: unknown scope {scope:?} (allowed: journeys:read, \
                     journeys:write, journeys:delete)",
                    client.name
                );
            };
            if !max_scopes.contains(&parsed) {
                max_scopes.push(parsed);
            }
        }
        ensure!(
            !max_scopes.is_empty(),
            "DELEGATED_CLIENTS: {}: maxScopes must list at least one scope",
            client.name
        );
        max_scopes.sort();
        clients.push(ClientConfig {
            name: client.name,
            display_name,
            issuer_url: client.issuer_url,
            audience: client.audience,
            max_scopes,
            enabled: client.enabled,
            service_group: client.service_group.trim().to_string(),
        });
    }
    Ok(clients)
}

pub(crate) fn valid_client_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The startup guard (§5.2): the SSO client, the internal client and every
/// delegated audience pairwise distinct, and no delegated issuer equal to
/// the SSO or internal issuer, so no token meant for one can satisfy
/// another's checks.
pub fn ensure_audiences_and_issuers_distinct(
    sso_client_id: &str,
    sso_issuer_url: &str,
    internal_client_id: &str,
    internal_issuer_url: &str,
    clients: &[ClientConfig],
) -> Result<()> {
    let mut audiences = vec![
        ("SSO_CLIENT_ID", sso_client_id.to_string()),
        ("INTERNAL_OAUTH_CLIENT_ID", internal_client_id.to_string()),
    ];
    for client in clients {
        ensure!(
            !same_issuer(&client.issuer_url, sso_issuer_url),
            "DELEGATED_CLIENTS: {}: issuerUrl equals SSO_ISSUER_URL; a delegated provider must \
             be its own Authentik provider",
            client.name
        );
        ensure!(
            !same_issuer(&client.issuer_url, internal_issuer_url),
            "DELEGATED_CLIENTS: {}: issuerUrl equals INTERNAL_OAUTH_ISSUER_URL",
            client.name
        );
        audiences.push(("DELEGATED_CLIENTS audience", client.audience.clone()));
    }
    for (i, (left_name, left)) in audiences.iter().enumerate() {
        for (right_name, right) in &audiences[i + 1..] {
            ensure!(
                left != right,
                "{left_name} and {right_name} are both {left:?}; every client id and delegated \
                 audience must be distinct, or a token for one would pass another's audience check"
            );
        }
    }
    let mut issuers = HashSet::new();
    for client in clients {
        ensure!(
            issuers.insert(client.issuer_url.trim_end_matches('/').to_string()),
            "DELEGATED_CLIENTS: two clients share issuerUrl {}",
            client.issuer_url
        );
    }
    Ok(())
}

fn same_issuer(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Settings from the environment (`crate::app::AppState::init`).
#[derive(Debug, Clone, PartialEq, Eq, clap::Parser)]
#[command(name = "api-delegated", no_binary_name = true)]
pub struct DelegatedSettings {
    /// The registered clients, a JSON list (chart `api.delegated.clients`).
    /// Empty (the default) makes the whole feature inert.
    #[arg(long, env = "DELEGATED_CLIENTS", default_value = "[]")]
    pub clients: String,
    /// The delegated listener (D-14). Bound only while a client is enabled.
    #[arg(long, env = "DELEGATED_LISTEN_ADDR", default_value = "0.0.0.0:8082")]
    pub listen_addr: String,
    /// Journeys one user's client may save per rolling 24 hours (D-2).
    #[arg(long, env = "DELEGATED_DAILY_SAVE_LIMIT", default_value_t = 50)]
    pub daily_save_limit: i64,
    /// Delegated deletes per user per rolling 24 hours (D-11).
    #[arg(long, env = "DELEGATED_DAILY_DELETE_LIMIT", default_value_t = 20)]
    pub daily_delete_limit: i64,
}

impl DelegatedSettings {
    pub fn from_env() -> Result<Self> {
        use clap::Parser;
        Self::try_parse_from(std::iter::empty::<String>())
            .context("invalid delegated-access settings in the environment")
    }
}

/// Verifies one client's tokens. Implemented by [`ServiceTokenVerifier`]
/// (production) and by test doubles.
pub(crate) trait DelegatedTokenCheck: Send + Sync + 'static {
    fn verify_signed<'a>(
        &'a self,
        token: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn Future<Output = Result<crate::auth::internal_oauth::SignedJwt, VerifyError>>
                + Send
                + 'a,
        >,
    >;
}

impl DelegatedTokenCheck for ServiceTokenVerifier {
    fn verify_signed<'a>(
        &'a self,
        token: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn Future<Output = Result<crate::auth::internal_oauth::SignedJwt, VerifyError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(self.verify_signed_jwt(token))
    }
}

/// A registered client and its verifier.
pub struct RegisteredClient {
    /// Position in `DELEGATED_CLIENTS`: the rate limiter's bucket key.
    pub index: u16,
    pub config: Arc<ClientConfig>,
    verifier: Box<dyn DelegatedTokenCheck>,
}

/// Every registered client, and the daily caps. `Default` is the inert
/// state: no client, no listener.
#[derive(Default)]
pub struct DelegatedAuth {
    clients: Vec<RegisteredClient>,
    listen_addr: String,
    pub daily_save_limit: i64,
    pub daily_delete_limit: i64,
}

impl std::fmt::Debug for DelegatedAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegatedAuth")
            .field(
                "clients",
                &self
                    .clients
                    .iter()
                    .map(|client| &client.config)
                    .collect::<Vec<_>>(),
            )
            .field("listen_addr", &self.listen_addr)
            .field("daily_save_limit", &self.daily_save_limit)
            .field("daily_delete_limit", &self.daily_delete_limit)
            .finish()
    }
}

impl DelegatedAuth {
    /// One [`ServiceTokenVerifier`] per client (each with its own JWKS
    /// cache, negative-`kid` cache and refresh cooldown).
    pub fn from_settings(settings: &DelegatedSettings, clients: Vec<ClientConfig>) -> Result<Self> {
        ensure!(
            settings.daily_save_limit > 0 && settings.daily_delete_limit > 0,
            "DELEGATED_DAILY_SAVE_LIMIT and DELEGATED_DAILY_DELETE_LIMIT must be at least 1"
        );
        let mut registered = Vec::with_capacity(clients.len());
        for (index, config) in clients.into_iter().enumerate() {
            let verifier =
                ServiceTokenVerifier::new(config.issuer_url.clone(), config.audience.clone())
                    .with_context(|| format!("delegated client {}", config.name))?;
            registered.push(RegisteredClient {
                index: u16::try_from(index).context("too many delegated clients")?,
                config: Arc::new(config),
                verifier: Box::new(verifier),
            });
        }
        Ok(Self {
            clients: registered,
            listen_addr: settings.listen_addr.clone(),
            daily_save_limit: settings.daily_save_limit,
            daily_delete_limit: settings.daily_delete_limit,
        })
    }

    /// No client registered: [`gate`] passes every request through.
    pub fn is_inert(&self) -> bool {
        self.clients.is_empty()
    }

    /// The delegated listener's address, when at least one client is
    /// enabled; `None` leaves the listener unbound.
    pub fn listen_addr(&self) -> Option<&str> {
        self.clients
            .iter()
            .any(|client| client.config.enabled)
            .then_some(self.listen_addr.as_str())
    }

    pub fn clients(&self) -> impl Iterator<Item = &Arc<ClientConfig>> {
        self.clients.iter().map(|client| &client.config)
    }

    /// The enabled client called `name`.
    pub fn enabled_client(&self, name: &str) -> Option<&Arc<ClientConfig>> {
        self.clients()
            .find(|client| client.name == name && client.enabled)
    }

    /// The client a token names by `iss` and `aud`, enabled or not.
    fn client_for(&self, issuer: &str, audience: &Audience) -> Option<&RegisteredClient> {
        self.clients.iter().find(|client| {
            client.config.issuer_url == issuer && audience.contains(&client.config.audience)
        })
    }
}

/// The claims of a delegated access token. `iat` is required: it is what
/// `not_before` and `sessions_invalidated_at` are compared with (§5.7).
#[derive(Debug, Clone, Deserialize)]
struct DelegatedClaims {
    sub: String,
    iss: String,
    aud: Audience,
    exp: i64,
    #[serde(default)]
    nbf: Option<i64>,
    iat: i64,
    #[serde(default)]
    jti: Option<String>,
    /// Space-separated, possibly empty (spike S4).
    #[serde(default)]
    scope: String,
}

/// The unverified `iss` and `aud` of a JWT, used only to pick a verifier.
#[derive(Debug, Deserialize)]
struct PeekedClaims {
    iss: String,
    aud: Audience,
}

fn peek_issuer(token: &str) -> Option<PeekedClaims> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A delegated token [`gate`] verified, for the rate limiter and the
/// extractor. Never holds the token itself.
#[derive(Debug, Clone)]
pub struct DelegatedToken {
    pub client: Arc<ClientConfig>,
    pub client_index: u16,
    pub sub: String,
    pub scopes: Vec<Scope>,
    pub iat: i64,
    pub jti: Option<String>,
    /// The scope this route needs ([`DELEGATED_ROUTES`]).
    pub route_scope: Scope,
    /// Filled by the extractor with the grant it resolved, for the audit
    /// line [`gate`] writes after the handler.
    pub(crate) audit: Arc<OnceLock<i64>>,
}

#[expect(
    clippy::cast_possible_wrap,
    reason = "small constant durations, far below i64::MAX seconds"
)]
async fn verify_delegated(
    client: &RegisteredClient,
    token: &str,
) -> Result<DelegatedClaims, VerifyError> {
    let signed = client.verifier.verify_signed(token).await?;
    if signed.typ.as_deref() == Some("logout+jwt") {
        return Err(VerifyError::Invalid);
    }
    let claims: DelegatedClaims =
        serde_json::from_slice(&signed.payload).map_err(|_| VerifyError::Malformed)?;
    if claims.iss != client.config.issuer_url || !claims.aud.contains(&client.config.audience) {
        return Err(VerifyError::Invalid);
    }
    let now = chrono::Utc::now().timestamp();
    let leeway = CLOCK_SKEW_LEEWAY.as_secs() as i64;
    if claims.exp.saturating_add(leeway) <= now {
        return Err(VerifyError::Invalid);
    }
    if let Some(nbf) = claims.nbf
        && nbf.saturating_sub(leeway) > now
    {
        return Err(VerifyError::Invalid);
    }
    if claims.iat.saturating_sub(leeway) > now {
        return Err(VerifyError::Invalid);
    }
    if claims.sub.is_empty() {
        return Err(VerifyError::Invalid);
    }
    Ok(claims)
}

/// The JSON error body of the delegated error contract (§5.15).
pub fn error_response(status: StatusCode, error: &'static str, message: &str) -> Response {
    let mut response = (
        status,
        axum::Json(serde_json::json!({ "error": error, "message": message })),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer error=\"invalid_token\""),
        );
    }
    response
}

/// A short, one-way digest for log lines (a `sub` or a session id), so a
/// log line can be correlated without carrying the value itself.
pub fn log_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn reject(
    client: Option<&str>,
    reason: &'static str,
    sub: Option<&str>,
    response: Response,
) -> Response {
    let client = client.unwrap_or("unknown").to_string();
    metrics::counter!(
        common::metrics::metric_name("api_delegated_rejected_total"),
        "client" => client.clone(),
        "reason" => reason
    )
    .increment(1);
    tracing::warn!(
        audit = true,
        event = "delegated_rejected",
        client = %client,
        reason,
        sub_hash = sub.map(log_hash),
        "delegated request rejected"
    );
    response
}

/// The router-wide middleware: see the module doc. Layer it outside the
/// rate limiter (`main.rs`).
pub async fn gate(
    State(auth): State<Arc<DelegatedAuth>>,
    mut request: Request,
    next: Next,
) -> Response {
    if auth.is_inert() {
        return next.run(request).await;
    }
    let Some(token) = bearer_token(request.headers()) else {
        return next.run(request).await;
    };
    // Rule 1 of §5.7: never pick one of a cookie and a bearer.
    if parse_cookie(request.headers(), SESSION_COOKIE_NAME).is_some() {
        return reject(
            None,
            "ambiguous_credentials",
            None,
            error_response(
                StatusCode::BAD_REQUEST,
                "ambiguous_credentials",
                "send either a session cookie or a bearer token, not both",
            ),
        );
    }
    let Some(peeked) = peek_issuer(&token) else {
        return next.run(request).await;
    };
    let Some(client) = auth.client_for(&peeked.iss, &peeked.aud) else {
        // Not a delegated token: the internal-token path, unchanged.
        return next.run(request).await;
    };
    let client_name = client.config.name.as_str();
    let invalid = || {
        error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "invalid bearer token",
        )
    };
    if request.extensions().get::<Ingress>() != Some(&Ingress::Delegated) {
        return reject(Some(client_name), "public_listener", None, invalid());
    }
    if !client.config.enabled {
        return reject(Some(client_name), "client_disabled", None, invalid());
    }
    let claims = match verify_delegated(client, &token).await {
        Ok(claims) => claims,
        Err(_) => return reject(Some(client_name), "invalid_token", None, invalid()),
    };
    let method = request.method().clone();
    let Some(matched) = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_string())
    else {
        return reject(
            Some(client_name),
            "route_not_delegated",
            Some(&claims.sub),
            error_response(StatusCode::NOT_FOUND, "not_found", "no such route"),
        );
    };
    let Some(scope) = route_scope(&method, &matched) else {
        return reject(
            Some(client_name),
            "route_not_delegated",
            Some(&claims.sub),
            error_response(
                StatusCode::FORBIDDEN,
                "insufficient_scope",
                "this route does not accept delegated tokens",
            ),
        );
    };
    let scopes: Vec<Scope> = claims
        .scope
        .split_ascii_whitespace()
        .filter_map(Scope::parse)
        .collect();
    let audit = Arc::new(OnceLock::new());
    let delegated = DelegatedToken {
        client: Arc::clone(&client.config),
        client_index: client.index,
        sub: claims.sub.clone(),
        scopes: scopes.clone(),
        iat: claims.iat,
        jti: claims.jti.clone(),
        route_scope: scope,
        audit: Arc::clone(&audit),
    };
    request.extensions_mut().insert(delegated);
    let response = next.run(request).await;
    let status = response.status();
    metrics::counter!(
        common::metrics::metric_name("api_delegated_requests_total"),
        "client" => client_name.to_string(),
        "route" => matched.clone(),
        "status" => status.as_u16().to_string()
    )
    .increment(1);
    tracing::info!(
        audit = true,
        event = "delegated_request",
        client = client_name,
        user_id = %claims.sub,
        grant_id = audit.get().copied(),
        jti = claims.jti.as_deref(),
        method = %method,
        route = %matched,
        status = status.as_u16(),
        scopes = %scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "),
        "delegated request"
    );
    response
}

/// The scope a handler needs, as a type ([`DelegatedOrSessionUser`]).
pub trait RequiredScope: Send + Sync + 'static {
    const SCOPE: Scope;
}

pub struct ReadScope;
impl RequiredScope for ReadScope {
    const SCOPE: Scope = Scope::JourneysRead;
}
pub struct WriteScope;
impl RequiredScope for WriteScope {
    const SCOPE: Scope = Scope::JourneysWrite;
}
pub struct DeleteScope;
impl RequiredScope for DeleteScope {
    const SCOPE: Scope = Scope::JourneysDelete;
}

/// How a [`DelegatedOrSessionUser`] was authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    Session,
    Delegated {
        client: Arc<ClientConfig>,
        grant_id: i64,
    },
}

/// The principal of an opted-in route: a session user, or a user whose
/// active grant lets a registered client act for them with scope `S`.
/// See the module doc and §5.7.
pub struct DelegatedOrSessionUser<S: RequiredScope> {
    pub user_id: String,
    pub via: Via,
    _scope: std::marker::PhantomData<S>,
}

impl<S: RequiredScope> DelegatedOrSessionUser<S> {
    /// The delegated client, if any.
    pub fn client(&self) -> Option<&Arc<ClientConfig>> {
        match &self.via {
            Via::Session => None,
            Via::Delegated { client, .. } => Some(client),
        }
    }

    pub fn is_delegated(&self) -> bool {
        matches!(self.via, Via::Delegated { .. })
    }
}

/// The effective scopes (§5.5): token ∩ grant ∩ the client's `maxScopes`.
pub fn effective_scopes(token: &[Scope], grant: &[String], max: &[Scope]) -> Vec<Scope> {
    let mut effective: Vec<Scope> = token
        .iter()
        .copied()
        .filter(|scope| grant.iter().any(|g| g == scope.as_str()) && max.contains(scope))
        .collect();
    effective.sort();
    effective.dedup();
    effective
}

impl<S: RequiredScope> FromRequestParts<App> for DelegatedOrSessionUser<S> {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        if let Some(token) = parts.extensions.get::<DelegatedToken>().cloned() {
            return resolve_delegated::<S>(app, &token).await;
        }
        // A bearer [`gate`] did not verify as delegated (an internal
        // service token, an unknown issuer, or the feature off): never fall
        // back to anything (§5.7 2.1).
        if bearer_token(&parts.headers).is_some() {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "invalid bearer token",
            ));
        }
        let user = AuthenticatedUser::from_request_parts(parts, app)
            .await
            .map_err(IntoResponse::into_response)?;
        Ok(Self {
            user_id: user.id,
            via: Via::Session,
            _scope: std::marker::PhantomData,
        })
    }
}

async fn resolve_delegated<S: RequiredScope>(
    app: &App,
    token: &DelegatedToken,
) -> Result<DelegatedOrSessionUser<S>, Response> {
    let client = token.client.name.as_str();
    let database_error = |err: anyhow::Error| {
        if let Some(unavailable) = crate::unavailable::response_for(&err) {
            return unavailable.into_response();
        }
        tracing::error!(error = ?err, "delegated grant lookup failed");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "grant lookup failed",
        )
    };
    let not_granted = || {
        error_response(
            StatusCode::FORBIDDEN,
            "delegation_not_granted",
            "the user has not granted this app access in Distant Signal",
        )
    };
    let Some(grant) = crate::data::delegated::active_grant(&app.database, &token.sub, client)
        .await
        .map_err(database_error)?
    else {
        return Err(reject(
            Some(client),
            "delegation_not_granted",
            Some(&token.sub),
            not_granted(),
        ));
    };
    let now = chrono::Utc::now();
    let last_use = grant.last_used_at.unwrap_or(grant.created_at);
    if now - last_use > chrono::Duration::days(GRANT_IDLE_DAYS) {
        // Expired before the hourly sweep got to it (D-6).
        if let Err(err) = crate::data::delegated::revoke_grant(
            &app.database,
            grant.id,
            crate::data::delegated::RevokeReason::Inactive,
        )
        .await
        {
            tracing::warn!(error = ?err, grant_id = grant.id, "could not mark an idle grant revoked");
        }
        return Err(reject(
            Some(client),
            "grant_inactive",
            Some(&token.sub),
            not_granted(),
        ));
    }
    let not_before = grant
        .sessions_invalidated_at
        .map_or(grant.not_before, |invalidated| {
            invalidated.max(grant.not_before)
        });
    if token.iat < not_before.timestamp() {
        return Err(reject(
            Some(client),
            "issued_before_not_before",
            Some(&token.sub),
            error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_token",
                "token issued before the grant",
            ),
        ));
    }
    let effective = effective_scopes(&token.scopes, &grant.scopes, &token.client.max_scopes);
    if !effective.contains(&S::SCOPE) {
        let mut response = error_response(
            StatusCode::FORBIDDEN,
            "insufficient_scope",
            &format!("this needs the {} scope", S::SCOPE.as_str()),
        );
        if let Ok(value) = HeaderValue::from_str(&format!(
            "Bearer error=\"insufficient_scope\", scope=\"{}\"",
            S::SCOPE.as_str()
        )) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        return Err(reject(
            Some(client),
            "insufficient_scope",
            Some(&token.sub),
            response,
        ));
    }
    if let Err(err) = crate::data::delegated::touch_grant(&app.database, grant.id).await {
        tracing::warn!(error = ?err, grant_id = grant.id, "could not record grant use");
    }
    let _ = token.audit.set(grant.id);
    Ok(DelegatedOrSessionUser {
        user_id: token.sub.clone(),
        via: Via::Delegated {
            client: Arc::clone(&token.client),
            grant_id: grant.id,
        },
        _scope: std::marker::PhantomData,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(name: &str, issuer: &str, audience: &str) -> ClientConfig {
        ClientConfig {
            name: name.to_string(),
            display_name: "Assistant".to_string(),
            issuer_url: issuer.to_string(),
            audience: audience.to_string(),
            max_scopes: Scope::ALL.to_vec(),
            enabled: true,
            service_group: "srv-ds-mcp".to_string(),
        }
    }

    #[test]
    fn parses_the_chart_shape() {
        let clients = parse_clients(
            r#"[{"name":"ds-mcp","displayName":"Distant Signal assistant",
                "issuerUrl":"https://sso.example/application/o/distant-signal-api/",
                "audience":"distant-signal-api",
                "maxScopes":["journeys:write","journeys:read","journeys:delete","journeys:read"],
                "serviceGroup":"srv-ds-mcp"}]"#,
        )
        .unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].name, "ds-mcp");
        assert!(clients[0].enabled, "enabled defaults to true");
        assert_eq!(
            clients[0].max_scopes,
            vec![
                Scope::JourneysRead,
                Scope::JourneysWrite,
                Scope::JourneysDelete
            ]
        );
    }

    #[test]
    fn empty_is_inert() {
        assert!(parse_clients("").unwrap().is_empty());
        assert!(parse_clients("[]").unwrap().is_empty());
        assert!(DelegatedAuth::default().is_inert());
        assert_eq!(DelegatedAuth::default().listen_addr(), None);
    }

    #[test]
    fn bad_entries_are_refused() {
        for json in [
            r#"[{"name":"Bad Name","displayName":"x","issuerUrl":"https://a/","audience":"a","maxScopes":["journeys:read"]}]"#,
            r#"[{"name":"web","displayName":"x","issuerUrl":"https://a/","audience":"a","maxScopes":["journeys:read"]}]"#,
            r#"[{"name":"a","displayName":"x","issuerUrl":"https://a/","audience":"a","maxScopes":["journeys:read_shared"]}]"#,
            r#"[{"name":"a","displayName":"x","issuerUrl":"https://a/","audience":"a","maxScopes":[]}]"#,
            r#"[{"name":"a","displayName":"","issuerUrl":"https://a/","audience":"a","maxScopes":["journeys:read"]}]"#,
            r#"[{"name":"a","displayName":"x","issuerUrl":"https://a/","audience":"","maxScopes":["journeys:read"]}]"#,
            r#"[{"name":"a","displayName":"x","issuerUrl":"https://a/","audience":"a","maxScopes":["journeys:read"],"actor":"x"}]"#,
            r#"[{"name":"a","displayName":"x","issuerUrl":"https://a/","audience":"a","maxScopes":["journeys:read"]},
                {"name":"a","displayName":"x","issuerUrl":"https://b/","audience":"b","maxScopes":["journeys:read"]}]"#,
        ] {
            assert!(parse_clients(json).is_err(), "{json}");
        }
    }

    #[test]
    fn the_startup_guard_keeps_every_audience_and_issuer_apart() {
        let ok = [client(
            "ds-mcp",
            "https://sso/application/o/distant-signal-api/",
            "distant-signal-api",
        )];
        ensure_audiences_and_issuers_distinct(
            "distant-signal",
            "https://sso/application/o/distant-signal/",
            "distant-signal-internal",
            "https://sso/application/o/distant-signal-internal/",
            &ok,
        )
        .unwrap();
        // The delegated audience equal to the SSO client id.
        let clash = [client(
            "ds-mcp",
            "https://sso/application/o/distant-signal-api/",
            "distant-signal",
        )];
        assert!(
            ensure_audiences_and_issuers_distinct(
                "distant-signal",
                "https://sso/application/o/distant-signal/",
                "distant-signal-internal",
                "https://sso/application/o/distant-signal-internal/",
                &clash,
            )
            .is_err()
        );
        // The delegated issuer equal to the SSO issuer (trailing slash aside).
        let clash = [client(
            "ds-mcp",
            "https://sso/application/o/distant-signal",
            "distant-signal-api",
        )];
        assert!(
            ensure_audiences_and_issuers_distinct(
                "distant-signal",
                "https://sso/application/o/distant-signal/",
                "distant-signal-internal",
                "https://sso/application/o/distant-signal-internal/",
                &clash,
            )
            .is_err()
        );
        // The delegated issuer equal to the internal issuer.
        let clash = [client(
            "ds-mcp",
            "https://sso/application/o/distant-signal-internal/",
            "distant-signal-api",
        )];
        assert!(
            ensure_audiences_and_issuers_distinct(
                "distant-signal",
                "https://sso/application/o/distant-signal/",
                "distant-signal-internal",
                "https://sso/application/o/distant-signal-internal/",
                &clash,
            )
            .is_err()
        );
        // The SSO and internal ids equal (the original two-client guard).
        assert!(
            ensure_audiences_and_issuers_distinct("same", "https://a/", "same", "https://b/", &[])
                .is_err()
        );
    }

    #[test]
    fn effective_scopes_are_the_intersection_of_all_three() {
        let token = [
            Scope::JourneysRead,
            Scope::JourneysWrite,
            Scope::JourneysDelete,
        ];
        let grant = vec!["journeys:read".to_string(), "journeys:delete".to_string()];
        let max = [Scope::JourneysRead, Scope::JourneysWrite];
        assert_eq!(
            effective_scopes(&token, &grant, &max),
            vec![Scope::JourneysRead]
        );
        // An empty token scope (spike S4) grants nothing.
        assert!(effective_scopes(&[], &grant, &max).is_empty());
    }

    #[test]
    fn route_scopes_cover_exactly_the_four_routes() {
        assert_eq!(
            route_scope(&Method::GET, "/Journeys/mine"),
            Some(Scope::JourneysRead)
        );
        assert_eq!(
            route_scope(&Method::GET, "/Journeys/{journey_id}"),
            Some(Scope::JourneysRead)
        );
        assert_eq!(
            route_scope(&Method::POST, "/Journeys/itinerary"),
            Some(Scope::JourneysWrite)
        );
        assert_eq!(
            route_scope(&Method::DELETE, "/Journeys/{journey_id}"),
            Some(Scope::JourneysDelete)
        );
        for (method, path) in [
            (Method::POST, "/Journeys"),
            (Method::POST, "/Journeys/{journey_id}/legs"),
            (Method::DELETE, "/Journeys/{journey_id}/legs/{leg_id}"),
            (Method::POST, "/Journeys/{journey_id}/share-link"),
            (Method::GET, "/Journeys/shared/{token}"),
            (Method::GET, "/public/groups/shared-journeys"),
            (Method::POST, "/public/admin/users/revoke-sessions"),
            (Method::GET, "/public/chatbot/access"),
            (Method::GET, "/public/account/export"),
            (Method::POST, "/public/account/connected-apps/{client}"),
            (Method::GET, "/Trips/plan"),
        ] {
            assert_eq!(route_scope(&method, path), None, "{method} {path}");
        }
    }

    #[test]
    fn log_hash_is_short_and_stable() {
        assert_eq!(log_hash("abc"), log_hash("abc"));
        assert_eq!(log_hash("abc").len(), 16);
        assert_ne!(log_hash("abc"), log_hash("abd"));
    }

    /// The gate in front of stand-in handlers: each says whether it ran and
    /// whether the gate left a verified [`DelegatedToken`] for it.
    mod gate_tests {
        use axum::body::Body;
        use axum::http::Request;
        use axum::routing::{get, post};
        use serde_json::json;
        use tower::ServiceExt;

        use super::*;
        use crate::auth::internal_oauth::test_support::{
            mock_authentik, sign_token, sign_token_with_typ,
        };

        const AUDIENCE: &str = "distant-signal-api";

        async fn handler(token: Option<axum::Extension<DelegatedToken>>) -> String {
            match token {
                Some(axum::Extension(token)) => format!("reached as {}", token.sub),
                None => "reached without a delegated token".to_string(),
            }
        }

        fn stand_in_routes() -> axum::Router {
            let public = axum::Router::new()
                .route("/groups/shared-journeys", get(handler))
                .route("/admin/users/revoke-sessions", post(handler))
                .route("/chatbot/access", get(handler))
                .route("/account/export", get(handler))
                .route("/account/connected-apps/{client}", post(handler));
            axum::Router::new()
                .route("/Journeys", post(handler))
                .route("/Journeys/mine", get(handler))
                .route("/Journeys/itinerary", post(handler))
                .route("/Journeys/{journey_id}", get(handler).delete(handler))
                .route("/Journeys/{journey_id}/share-link", post(handler))
                .route("/Journeys/{journey_id}/legs", post(handler))
                .route("/Train/track", post(handler))
                .route("/delegated-grants/revoke", post(handler))
                .nest("/public", public)
        }

        async fn auth_for(issuer: &str, enabled: bool) -> Arc<DelegatedAuth> {
            let settings = DelegatedSettings {
                clients: String::new(),
                listen_addr: "127.0.0.1:0".to_string(),
                daily_save_limit: 50,
                daily_delete_limit: 20,
            };
            let mut config = client("ds-mcp", issuer, AUDIENCE);
            config.enabled = enabled;
            Arc::new(DelegatedAuth::from_settings(&settings, vec![config]).unwrap())
        }

        fn routers(auth: Arc<DelegatedAuth>) -> (axum::Router, axum::Router) {
            let router = stand_in_routes().layer(axum::middleware::from_fn_with_state(auth, gate));
            (router.clone(), delegated_listener_router(router))
        }

        fn claims(issuer: &str, mutate: impl FnOnce(&mut serde_json::Value)) -> serde_json::Value {
            let now = chrono::Utc::now().timestamp();
            let mut claims = json!({
                "sub": "user-1",
                "iss": issuer,
                "aud": AUDIENCE,
                "azp": AUDIENCE,
                "iat": now,
                "exp": now + 300,
                "jti": "jti-1",
                "scope": "journeys:write journeys:read journeys:delete",
            });
            mutate(&mut claims);
            claims
        }

        async fn send(
            router: &axum::Router,
            method: &str,
            uri: &str,
            token: &str,
            cookie: bool,
        ) -> (StatusCode, HeaderMap, String) {
            let mut request = Request::builder()
                .method(method)
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"));
            if cookie {
                request = request.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}=abc"));
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let body = axum::body::to_bytes(response.into_body(), 1 << 16)
                .await
                .unwrap();
            (status, headers, String::from_utf8(body.to_vec()).unwrap())
        }

        use axum::http::HeaderMap;

        #[tokio::test]
        async fn a_delegated_token_works_only_on_the_delegated_listener() {
            let (server, _) = mock_authentik().await;
            let (public, delegated) = routers(auth_for(&server.uri(), true).await);
            let token = sign_token(&claims(&server.uri(), |_| {}));

            let (status, headers, body) =
                send(&public, "GET", "/Journeys/mine", &token, false).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
            assert!(body.contains("invalid_token"), "{body}");
            assert_eq!(
                headers[header::WWW_AUTHENTICATE],
                "Bearer error=\"invalid_token\""
            );

            let (status, _, body) = send(&delegated, "GET", "/Journeys/mine", &token, false).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body, "reached as user-1");
            for (method, uri) in [
                ("GET", "/Journeys/42"),
                ("POST", "/Journeys/itinerary"),
                ("DELETE", "/Journeys/42"),
            ] {
                let (status, _, body) = send(&delegated, method, uri, &token, false).await;
                assert_eq!(status, StatusCode::OK, "{method} {uri}: {body}");
                assert_eq!(body, "reached as user-1", "{method} {uri}");
            }
        }

        #[tokio::test]
        async fn delegated_tokens_never_reach_any_other_route() {
            let (server, _) = mock_authentik().await;
            let (_, delegated) = routers(auth_for(&server.uri(), true).await);
            let token = sign_token(&claims(&server.uri(), |_| {}));
            for (method, uri) in [
                ("POST", "/Journeys"),
                ("POST", "/Journeys/42/legs"),
                ("POST", "/Journeys/42/share-link"),
                ("POST", "/Train/track"),
                ("POST", "/delegated-grants/revoke"),
                ("GET", "/public/groups/shared-journeys"),
                ("POST", "/public/admin/users/revoke-sessions"),
                ("GET", "/public/chatbot/access"),
                ("GET", "/public/account/export"),
                ("POST", "/public/account/connected-apps/ds-mcp"),
            ] {
                let (status, _, body) = send(&delegated, method, uri, &token, false).await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {body}");
                assert!(
                    body.contains("insufficient_scope"),
                    "{method} {uri}: {body}"
                );
            }
        }

        #[tokio::test]
        async fn a_cookie_and_a_bearer_together_are_refused() {
            let (server, _) = mock_authentik().await;
            let (public, delegated) = routers(auth_for(&server.uri(), true).await);
            let token = sign_token(&claims(&server.uri(), |_| {}));
            for router in [&public, &delegated] {
                let (status, _, body) = send(router, "GET", "/Journeys/mine", &token, true).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
                assert!(body.contains("ambiguous_credentials"), "{body}");
            }
        }

        #[tokio::test]
        async fn the_kill_switch_refuses_every_token() {
            let (server, _) = mock_authentik().await;
            let auth = auth_for(&server.uri(), false).await;
            assert_eq!(
                auth.listen_addr(),
                None,
                "no listener for a disabled client"
            );
            let (_, delegated) = routers(auth);
            let token = sign_token(&claims(&server.uri(), |_| {}));
            let (status, _, body) = send(&delegated, "GET", "/Journeys/mine", &token, false).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        }

        #[tokio::test]
        async fn bad_tokens_for_the_client_are_refused() {
            let (server, _) = mock_authentik().await;
            let (_, delegated) = routers(auth_for(&server.uri(), true).await);
            let uri = server.uri();
            let cases = [
                (
                    "expired",
                    sign_token(&claims(&uri, |c| {
                        c["exp"] = json!(chrono::Utc::now().timestamp() - 3600);
                    })),
                ),
                (
                    "no iat",
                    sign_token(&claims(&uri, |c| {
                        c.as_object_mut().unwrap().remove("iat");
                    })),
                ),
                (
                    "issued in the future",
                    sign_token(&claims(&uri, |c| {
                        c["iat"] = json!(chrono::Utc::now().timestamp() + 3600);
                    })),
                ),
                (
                    "a logout token",
                    sign_token_with_typ(&claims(&uri, |_| {}), "logout+jwt"),
                ),
                ("a bad signature", {
                    let token = sign_token(&claims(&uri, |_| {}));
                    let (rest, _) = token.rsplit_once('.').unwrap();
                    format!("{rest}.AAAA")
                }),
            ];
            for (what, token) in cases {
                let (status, _, body) =
                    send(&delegated, "GET", "/Journeys/mine", &token, false).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{what}: {body}");
            }
        }

        #[tokio::test]
        async fn other_audiences_are_never_treated_as_delegated() {
            let (server, _) = mock_authentik().await;
            let (_, delegated) = routers(auth_for(&server.uri(), true).await);
            // The same issuer with the SSO client's audience, and another
            // issuer with the delegated audience: neither names the client,
            // so the gate leaves them alone and no handler sees a delegated
            // identity (the extractor then answers 401).
            for token in [
                sign_token(&claims(&server.uri(), |c| {
                    c["aud"] = json!("distant-signal")
                })),
                sign_token(&claims("https://other.example/", |_| {})),
            ] {
                let (status, _, body) =
                    send(&delegated, "GET", "/Journeys/mine", &token, false).await;
                assert_eq!(status, StatusCode::OK);
                assert_eq!(body, "reached without a delegated token");
            }
        }

        /// The pinned list of handlers that take [`DelegatedOrSessionUser`]
        /// (spec §5.7): a handler added to (or removed from) it fails here
        /// until [`DELEGATED_ROUTES`] and this list agree.
        #[test]
        fn the_delegated_handlers_are_pinned() {
            assert_eq!(
                DELEGATED_ROUTES
                    .iter()
                    .map(|(method, path, scope)| format!("{method} {path} {}", scope.as_str()))
                    .collect::<Vec<_>>(),
                [
                    "GET /Journeys/mine journeys:read",
                    "GET /Journeys/{journey_id} journeys:read",
                    "POST /Journeys/itinerary journeys:write",
                    "DELETE /Journeys/{journey_id} journeys:delete",
                ]
            );
            let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
            let mut found = Vec::new();
            let mut stack = vec![src.clone()];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|ext| ext == "rs")
                        // This module defines the extractor (and this test
                        // names it); only its users are counted.
                        && !path.ends_with("auth/delegated.rs")
                    {
                        let text = std::fs::read_to_string(&path).unwrap();
                        let file = path.strip_prefix(&src).unwrap().display().to_string();
                        for (index, _) in text.match_indices("user: DelegatedOrSessionUser<") {
                            let scope: String = text
                                [index + "user: DelegatedOrSessionUser<".len()..]
                                .chars()
                                .take_while(char::is_ascii_alphanumeric)
                                .collect();
                            found.push(format!("{file} {scope}"));
                        }
                    }
                }
            }
            found.sort();
            assert_eq!(
                found,
                [
                    "routes/journey_itinerary.rs WriteScope",
                    "routes/journeys.rs DeleteScope",
                    "routes/journeys.rs ReadScope",
                    "routes/journeys.rs ReadScope",
                ],
                "the handlers taking DelegatedOrSessionUser changed; update DELEGATED_ROUTES, \
                 the spec's §5.8 and this list together"
            );
        }
    }
}
