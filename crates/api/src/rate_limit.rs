//! Per-client-IP rate limits on the api's expensive or write-amplifying
//! public routes (API-5, TRIPS-1; the user's revised DQ3 decision).
//!
//! Production has no ingress-nginx: public traffic arrives Cloudflare tunnel
//! -> cloudflared -> frontend -> api. So the socket peer of almost every
//! request is the frontend pod, and the real client is only known from the
//! `X-Real-IP` header the frontend proxy sets (from `CF-Connecting-IP`,
//! overwriting whatever the client sent). The key is therefore:
//!
//! - `X-Real-IP`, when [`RateLimitSettings::trust_x_real_ip`] is on (the
//!   default: the api Service is ClusterIP-only, so only in-cluster pods can
//!   set it);
//! - otherwise the socket peer address.
//!
//! `X-Forwarded-For` is never read: a client can put anything in it. A
//! request with no usable `X-Real-IP` falls back to the peer, which is
//! usually the frontend pod, so all such requests share one bucket; that is
//! counted in `distant_signal_api_rate_limit_peer_fallback_total` and logged
//! once, so it is visible. IPv6 clients are keyed by their /64.
//!
//! Limited ([`classify`]):
//!
//! - login: `/public/auth/login` and `/public/auth/callback` (each login
//!   writes an `oidc_login_state` row and sweeps expired ones);
//! - `/Trips/plan` (the most expensive anonymous read);
//! - `/Train/by-uid/*` (anonymous, and a cache miss runs schedule matching);
//! - every other public write (non-GET/HEAD/OPTIONS), with a generous
//!   default, as a backstop for anonymous write amplification.
//!
//! `/private/*` (internal OAuth ingest) is never limited. A limited request
//! gets `429 Too Many Requests` with `Retry-After`, and is counted in
//! `distant_signal_api_rate_limited_total{class,caller}`.
//!
//! ## Trusted service callers (the MCP)
//!
//! The Distant-Signal-MCP calls these public routes in-cluster, directly
//! (not through the frontend), so it has no `X-Real-IP` and would otherwise
//! share one small per-peer bucket for all of its users. It is recognised by
//! the same internal OAuth machinery `/private/*` uses: an Authentik
//! client-credentials JWT, verified against the cached JWKS by the api's one
//! shared [`ServiceTokenVerifier`](crate::auth::internal_oauth::ServiceTokenVerifier),
//! whose `groups` claim contains `INTERNAL_OAUTH_GROUP_MCP`. Such a request
//! is NOT exempt: it is charged to a separate, finite budget
//! (`API_RATE_LIMIT_MCP_*`, 5x the public defaults), keyed on the caller
//! identity (`svc:mcp`, one bucket for the whole service, never per IP), so
//! a runaway loop or a leaked credential is still bounded.
//!
//! The rules ([`ServiceCallerAuth`], [`bearer_decision`]):
//!
//! - no `Authorization: Bearer` header: anonymous, per IP, exactly as above
//!   (the frontend proxy never forwards an `Authorization` header, so the
//!   browser path never pays for token verification);
//! - a bearer that verifies AND carries the MCP group: the MCP budget;
//! - a bearer that fails verification (malformed, expired, bad signature,
//!   wrong issuer or audience): `401`, never a silent fall-back to the
//!   anonymous bucket. It is rejected before any handler runs, so there is
//!   nothing to bypass, and a misconfigured MCP fails loudly instead of
//!   being quietly squeezed into the in-cluster peer's tiny bucket;
//! - a bearer that verifies but lacks the MCP group (another service's
//!   credential): `403`, same reasoning; logged with its `sub`;
//! - login routes ignore the bearer entirely (the MCP has no business
//!   logging in; they keep the anonymous per-IP limit);
//! - `INTERNAL_OAUTH_GROUP_MCP` empty: the feature is inert and any bearer
//!   is ignored (anonymous, per IP, as before this existed).
//!
//! The bearer is read only here. End-user auth on public routes is the
//! `distant_signal_session` cookie (`crate::auth::AuthenticatedUser`), which
//! never looks at `Authorization`, so the two cannot collide: an MCP request
//! is still anonymous as far as every handler is concerned.
//!
//! The algorithm is GCRA (a token bucket kept as one timestamp per key):
//! `per_minute` sustained, `burst` at once. State is per pod, in memory.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result, ensure};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::internal_oauth::{ServiceClaims, VerifyError};

/// The header the frontend proxy sets to the real client address.
pub const REAL_IP_HEADER: &str = "x-real-ip";

/// Which limit a request falls under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitClass {
    Login,
    TripPlan,
    TrainByUid,
    PublicWrite,
}

impl LimitClass {
    pub fn label(self) -> &'static str {
        match self {
            LimitClass::Login => "login",
            LimitClass::TripPlan => "trip_plan",
            LimitClass::TrainByUid => "train_by_uid",
            LimitClass::PublicWrite => "public_write",
        }
    }
}

/// Who a limited request is charged to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Caller {
    /// Anonymous (or a signed-in end user): keyed per client IP.
    Public,
    /// The Distant-Signal-MCP, proven by an internal OAuth token: one
    /// bucket for the whole service.
    Mcp,
}

impl Caller {
    pub fn label(self) -> &'static str {
        match self {
            Caller::Public => "public",
            Caller::Mcp => "mcp",
        }
    }
}

/// A limiter bucket key: a client IP, or a service identity (`svc:mcp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKey {
    Ip(IpAddr),
    Service(Caller),
}

impl ClientKey {
    fn caller(self) -> Caller {
        match self {
            ClientKey::Ip(_) => Caller::Public,
            ClientKey::Service(caller) => caller,
        }
    }
}

impl std::fmt::Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientKey::Ip(ip) => write!(f, "{ip}"),
            ClientKey::Service(caller) => write!(f, "svc:{}", caller.label()),
        }
    }
}

/// The limit class of a request, or `None` for an unlimited one.
/// `path` is the full request path as the top-level router sees it.
pub fn classify(method: &Method, path: &str) -> Option<LimitClass> {
    if path == "/private" || path.starts_with("/private/") {
        return None;
    }
    if path == "/public/auth/login" || path == "/public/auth/callback" {
        return Some(LimitClass::Login);
    }
    if path == "/Trips/plan" {
        return Some(LimitClass::TripPlan);
    }
    if path.starts_with("/Train/by-uid/") {
        return Some(LimitClass::TrainByUid);
    }
    if !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Some(LimitClass::PublicWrite);
    }
    None
}

/// Rate-limit settings, from the environment.
#[derive(Debug, Clone, PartialEq, Eq, clap::Parser)]
#[command(name = "api-rate-limit", no_binary_name = true)]
pub struct RateLimitSettings {
    /// Master switch.
    #[arg(long, env = "API_RATE_LIMIT_ENABLED", default_value_t = true, action = clap::ArgAction::Set)]
    pub enabled: bool,
    /// Key on `X-Real-IP` when present. Only safe while every client reaches
    /// the api through a proxy that overwrites it (the frontend); the api
    /// Service is ClusterIP-only.
    #[arg(long, env = "API_RATE_LIMIT_TRUST_X_REAL_IP", default_value_t = true, action = clap::ArgAction::Set)]
    pub trust_x_real_ip: bool,

    #[arg(long, env = "API_RATE_LIMIT_LOGIN_PER_MINUTE", default_value_t = 10)]
    pub login_per_minute: u32,
    #[arg(long, env = "API_RATE_LIMIT_LOGIN_BURST", default_value_t = 20)]
    pub login_burst: u32,

    #[arg(
        long,
        env = "API_RATE_LIMIT_TRIP_PLAN_PER_MINUTE",
        default_value_t = 20
    )]
    pub trip_plan_per_minute: u32,
    #[arg(long, env = "API_RATE_LIMIT_TRIP_PLAN_BURST", default_value_t = 10)]
    pub trip_plan_burst: u32,

    #[arg(
        long,
        env = "API_RATE_LIMIT_TRAIN_BY_UID_PER_MINUTE",
        default_value_t = 120
    )]
    pub train_by_uid_per_minute: u32,
    #[arg(long, env = "API_RATE_LIMIT_TRAIN_BY_UID_BURST", default_value_t = 60)]
    pub train_by_uid_burst: u32,

    #[arg(
        long,
        env = "API_RATE_LIMIT_PUBLIC_WRITE_PER_MINUTE",
        default_value_t = 120
    )]
    pub public_write_per_minute: u32,
    #[arg(long, env = "API_RATE_LIMIT_PUBLIC_WRITE_BURST", default_value_t = 60)]
    pub public_write_burst: u32,

    /// The MCP's own budget (one bucket for the whole service, see the
    /// module doc). Login has none: the bearer is ignored there.
    #[arg(
        long,
        env = "API_RATE_LIMIT_MCP_TRIP_PLAN_PER_MINUTE",
        default_value_t = 100
    )]
    pub mcp_trip_plan_per_minute: u32,
    #[arg(long, env = "API_RATE_LIMIT_MCP_TRIP_PLAN_BURST", default_value_t = 30)]
    pub mcp_trip_plan_burst: u32,
    #[arg(
        long,
        env = "API_RATE_LIMIT_MCP_TRAIN_BY_UID_PER_MINUTE",
        default_value_t = 600
    )]
    pub mcp_train_by_uid_per_minute: u32,
    #[arg(
        long,
        env = "API_RATE_LIMIT_MCP_TRAIN_BY_UID_BURST",
        default_value_t = 200
    )]
    pub mcp_train_by_uid_burst: u32,
    #[arg(
        long,
        env = "API_RATE_LIMIT_MCP_PUBLIC_WRITE_PER_MINUTE",
        default_value_t = 600
    )]
    pub mcp_public_write_per_minute: u32,
    #[arg(
        long,
        env = "API_RATE_LIMIT_MCP_PUBLIC_WRITE_BURST",
        default_value_t = 200
    )]
    pub mcp_public_write_burst: u32,
}

impl Default for RateLimitSettings {
    /// The declared defaults (kept equal by
    /// `tests::defaults_match_the_declared_values`).
    fn default() -> Self {
        Self {
            enabled: true,
            trust_x_real_ip: true,
            login_per_minute: 10,
            login_burst: 20,
            trip_plan_per_minute: 20,
            trip_plan_burst: 10,
            train_by_uid_per_minute: 120,
            train_by_uid_burst: 60,
            public_write_per_minute: 120,
            public_write_burst: 60,
            mcp_trip_plan_per_minute: 100,
            mcp_trip_plan_burst: 30,
            mcp_train_by_uid_per_minute: 600,
            mcp_train_by_uid_burst: 200,
            mcp_public_write_per_minute: 600,
            mcp_public_write_burst: 200,
        }
    }
}

impl RateLimitSettings {
    pub fn from_env() -> Result<Self> {
        use clap::Parser;
        let settings = Self::try_parse_from(std::iter::empty::<String>())
            .context("invalid api rate-limit settings in the environment")?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        for (class, (per_minute, burst)) in [
            (LimitClass::Login, (self.login_per_minute, self.login_burst)),
            (
                LimitClass::TripPlan,
                (self.trip_plan_per_minute, self.trip_plan_burst),
            ),
            (
                LimitClass::TrainByUid,
                (self.train_by_uid_per_minute, self.train_by_uid_burst),
            ),
            (
                LimitClass::PublicWrite,
                (self.public_write_per_minute, self.public_write_burst),
            ),
        ] {
            ensure!(
                per_minute > 0 && burst > 0,
                "the {} rate limit needs a per-minute rate and a burst of at least 1",
                class.label()
            );
        }
        for (class, (per_minute, burst)) in [
            (
                LimitClass::TripPlan,
                (self.mcp_trip_plan_per_minute, self.mcp_trip_plan_burst),
            ),
            (
                LimitClass::TrainByUid,
                (
                    self.mcp_train_by_uid_per_minute,
                    self.mcp_train_by_uid_burst,
                ),
            ),
            (
                LimitClass::PublicWrite,
                (
                    self.mcp_public_write_per_minute,
                    self.mcp_public_write_burst,
                ),
            ),
        ] {
            ensure!(
                per_minute > 0 && burst > 0,
                "the MCP {} rate limit needs a per-minute rate and a burst of at least 1 \
                 (it is a finite budget, never an exemption)",
                class.label()
            );
        }
        Ok(())
    }

    fn quota(&self, class: LimitClass, caller: Caller) -> Quota {
        let (per_minute, burst) = match (caller, class) {
            (_, LimitClass::Login) => (self.login_per_minute, self.login_burst),
            (Caller::Public, LimitClass::TripPlan) => {
                (self.trip_plan_per_minute, self.trip_plan_burst)
            }
            (Caller::Public, LimitClass::TrainByUid) => {
                (self.train_by_uid_per_minute, self.train_by_uid_burst)
            }
            (Caller::Public, LimitClass::PublicWrite) => {
                (self.public_write_per_minute, self.public_write_burst)
            }
            (Caller::Mcp, LimitClass::TripPlan) => {
                (self.mcp_trip_plan_per_minute, self.mcp_trip_plan_burst)
            }
            (Caller::Mcp, LimitClass::TrainByUid) => (
                self.mcp_train_by_uid_per_minute,
                self.mcp_train_by_uid_burst,
            ),
            (Caller::Mcp, LimitClass::PublicWrite) => (
                self.mcp_public_write_per_minute,
                self.mcp_public_write_burst,
            ),
        };
        Quota::new(per_minute, burst)
    }
}

#[derive(Debug, Clone, Copy)]
struct Quota {
    /// Time one request "costs".
    interval: Duration,
    /// How far ahead of now the bucket may run: `interval * burst`.
    tolerance: Duration,
}

impl Quota {
    fn new(per_minute: u32, burst: u32) -> Self {
        let interval = Duration::from_secs(60) / per_minute.max(1);
        Self {
            interval,
            tolerance: interval * burst.max(1),
        }
    }
}

/// Past this many tracked keys, fully recovered ones are pruned; if that
/// isn't enough, the table is cleared (fail open) rather than grow without
/// bound under a spoofed-address flood.
const MAX_TRACKED_KEYS: usize = 100_000;
/// Prune fully recovered keys every this many checks.
const PRUNE_EVERY: u64 = 4096;

/// Verifies an internal OAuth bearer token. Implemented by
/// [`AppState`](crate::app::AppState) (delegating to its one shared
/// `internal_oauth_verifier`, so this path and `/private/*` share a single
/// JWKS cache, negative-`kid` cache and refetch cooldown) and by
/// `ServiceTokenVerifier` itself (tests).
pub trait ServiceTokenCheck: Send + Sync + 'static {
    fn verify_service_token<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ServiceClaims, VerifyError>> + Send + 'a>>;
}

impl ServiceTokenCheck for crate::auth::internal_oauth::ServiceTokenVerifier {
    fn verify_service_token<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ServiceClaims, VerifyError>> + Send + 'a>> {
        Box::pin(self.verify(token))
    }
}

impl ServiceTokenCheck for crate::app::AppState {
    fn verify_service_token<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ServiceClaims, VerifyError>> + Send + 'a>> {
        Box::pin(self.internal_oauth_verifier.verify(token))
    }
}

/// How the limiter recognises the MCP: see the module doc.
pub struct ServiceCallerAuth {
    verifier: Arc<dyn ServiceTokenCheck>,
    mcp_group: String,
}

impl ServiceCallerAuth {
    /// `None` when `mcp_group` is blank: the feature is then inert.
    pub fn new(verifier: Arc<dyn ServiceTokenCheck>, mcp_group: &str) -> Option<Self> {
        let mcp_group = mcp_group.trim();
        (!mcp_group.is_empty()).then(|| Self {
            verifier,
            mcp_group: mcp_group.to_string(),
        })
    }
}

/// What a verified-or-not bearer token earns on a limited route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BearerDecision {
    /// Charge the MCP's own budget.
    Mcp,
    /// Refuse with this status; never fall back to the anonymous bucket.
    Reject(StatusCode),
}

/// The pure half of the bearer check: `verified` is the verifier's result.
/// Only a token that verifies AND lists `mcp_group` earns the MCP budget.
pub fn bearer_decision(
    verified: &Result<ServiceClaims, VerifyError>,
    mcp_group: &str,
) -> BearerDecision {
    match verified {
        Err(_) => BearerDecision::Reject(StatusCode::UNAUTHORIZED),
        Ok(claims) if !mcp_group.is_empty() && claims.groups.iter().any(|g| g == mcp_group) => {
            BearerDecision::Mcp
        }
        Ok(_) => BearerDecision::Reject(StatusCode::FORBIDDEN),
    }
}

/// The limiter: one GCRA timestamp per (class, client key).
pub struct RateLimiter {
    settings: RateLimitSettings,
    service_callers: Option<ServiceCallerAuth>,
    state: Mutex<LimiterState>,
    warned_about_peer_fallback: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct LimiterState {
    /// Theoretical arrival time per key: the bucket is empty until then.
    tat: HashMap<(LimitClass, ClientKey), Instant>,
    checks: u64,
}

/// Where a request's key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    RealIpHeader,
    Peer,
}

impl RateLimiter {
    /// A limiter with no trusted service callers: every bearer is ignored.
    pub fn new(settings: RateLimitSettings) -> Arc<Self> {
        Self::with_service_callers(settings, None)
    }

    /// A limiter that recognises the MCP (see the module doc); `None` is
    /// the same as [`RateLimiter::new`].
    pub fn with_service_callers(
        settings: RateLimitSettings,
        service_callers: Option<ServiceCallerAuth>,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            service_callers,
            state: Mutex::new(LimiterState::default()),
            warned_about_peer_fallback: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// `Ok` if a request by `client` in `class` is allowed at `now` (and
    /// records it), else `Err(retry_after)`.
    #[expect(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "a poisoned lock means another thread already panicked"
    )]
    fn check_at(&self, class: LimitClass, client: ClientKey, now: Instant) -> Result<(), Duration> {
        let quota = self.settings.quota(class, client.caller());
        let mut state = self.state.lock().expect("rate limiter lock poisoned");
        state.checks += 1;
        if state.checks.is_multiple_of(PRUNE_EVERY) || state.tat.len() >= MAX_TRACKED_KEYS {
            state.tat.retain(|_, tat| *tat > now);
            if state.tat.len() >= MAX_TRACKED_KEYS {
                tracing::warn!(
                    keys = state.tat.len(),
                    "rate limiter key table full; clearing it"
                );
                metrics::counter!(common::metrics::metric_name(
                    "api_rate_limit_table_resets_total"
                ))
                .increment(1);
                state.tat.clear();
            }
        }
        let key = (class, client);
        let tat = state.tat.get(&key).copied().unwrap_or(now).max(now);
        let new_tat = tat + quota.interval;
        let ahead = new_tat - now;
        if ahead > quota.tolerance {
            return Err(ahead.checked_sub(quota.tolerance).unwrap());
        }
        state.tat.insert(key, new_tat);
        Ok(())
    }

    /// Who a limited request is charged to, or the rejection for a bad
    /// bearer. `Ok(None)`: anonymous, key it per IP. See the module doc.
    async fn service_caller(
        &self,
        class: LimitClass,
        headers: &HeaderMap,
    ) -> Result<Option<Caller>, Box<Response>> {
        let Some(auth) = &self.service_callers else {
            return Ok(None);
        };
        if class == LimitClass::Login {
            return Ok(None);
        }
        let Some(token) = crate::auth::bearer_token(headers) else {
            return Ok(None);
        };
        let verified = auth.verifier.verify_service_token(&token).await;
        match bearer_decision(&verified, &auth.mcp_group) {
            BearerDecision::Mcp => Ok(Some(Caller::Mcp)),
            BearerDecision::Reject(status) => {
                let reason = if status == StatusCode::UNAUTHORIZED {
                    "invalid_token"
                } else {
                    "wrong_group"
                };
                metrics::counter!(
                    common::metrics::metric_name("api_rate_limit_service_auth_rejected_total"),
                    "class" => class.label(),
                    "reason" => reason
                )
                .increment(1);
                Err(Box::new(if let Ok(claims) = verified {
                    tracing::warn!(
                        sub = %claims.sub,
                        class = class.label(),
                        "valid internal oauth token without the MCP group on a public \
                         rate-limited route; rejected 403"
                    );
                    (
                        StatusCode::FORBIDDEN,
                        "this service credential is not allowed on public routes",
                    )
                        .into_response()
                } else {
                    let mut response =
                        (StatusCode::UNAUTHORIZED, "invalid bearer token").into_response();
                    response.headers_mut().insert(
                        header::WWW_AUTHENTICATE,
                        HeaderValue::from_static("Bearer error=\"invalid_token\""),
                    );
                    response
                }))
            }
        }
    }

    /// The key for a request: see the module doc.
    pub fn client_key(
        &self,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> Option<(IpAddr, KeySource)> {
        if self.settings.trust_x_real_ip
            && let Some(ip) = headers
                .get(REAL_IP_HEADER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<IpAddr>().ok())
        {
            return Some((normalise(ip), KeySource::RealIpHeader));
        }
        peer.map(|peer| (normalise(peer.ip()), KeySource::Peer))
    }

    fn note_peer_fallback(&self, class: LimitClass) {
        metrics::counter!(
            common::metrics::metric_name("api_rate_limit_peer_fallback_total"),
            "class" => class.label()
        )
        .increment(1);
        if !self
            .warned_about_peer_fallback
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            tracing::warn!(
                "a rate-limited request had no usable X-Real-IP header, so it was keyed on the \
                 socket peer (usually the frontend pod, shared by every such client); logged \
                 once, counted in distant_signal_api_rate_limit_peer_fallback_total"
            );
        }
    }
}

/// IPv4-mapped IPv6 as IPv4; other IPv6 cut to its /64, since one client
/// usually holds a whole /64.
fn normalise(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                IpAddr::V4(v4)
            } else {
                let bits = u128::from(v6) & !((1u128 << 64) - 1);
                IpAddr::V6(Ipv6Addr::from(bits))
            }
        }
    }
}

/// The middleware. Layer it on the top-level router with
/// `axum::middleware::from_fn_with_state(limiter, rate_limit::enforce)`.
pub async fn enforce(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    if !limiter.settings.enabled {
        return next.run(request).await;
    }
    let Some(class) = classify(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };
    let client = match limiter.service_caller(class, request.headers()).await {
        Err(rejection) => return *rejection,
        Ok(Some(caller)) => ClientKey::Service(caller),
        Ok(None) => {
            let peer = request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| *addr);
            let Some((client, source)) = limiter.client_key(request.headers(), peer) else {
                // No header and no peer: only in tests that bypass the listener.
                return next.run(request).await;
            };
            if source == KeySource::Peer && limiter.settings.trust_x_real_ip {
                limiter.note_peer_fallback(class);
            }
            ClientKey::Ip(client)
        }
    };
    match limiter.check_at(class, client, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            let caller = client.caller();
            metrics::counter!(
                common::metrics::metric_name("api_rate_limited_total"),
                "class" => class.label(),
                "caller" => caller.label()
            )
            .increment(1);
            if caller != Caller::Public {
                tracing::debug!(%client, class = class.label(), "service caller rate limited");
            }
            too_many_requests(retry_after)
        }
    }
}

fn too_many_requests(retry_after: Duration) -> Response {
    let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        "too many requests; please slow down",
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(secs.max(1)));
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::routing::{get, post};
    use tower::ServiceExt;

    use super::*;

    fn settings() -> RateLimitSettings {
        RateLimitSettings {
            login_per_minute: 60,
            login_burst: 2,
            trip_plan_per_minute: 60,
            trip_plan_burst: 2,
            train_by_uid_per_minute: 60,
            train_by_uid_burst: 2,
            public_write_per_minute: 60,
            public_write_burst: 2,
            mcp_trip_plan_per_minute: 60,
            mcp_trip_plan_burst: 5,
            mcp_train_by_uid_per_minute: 60,
            mcp_train_by_uid_burst: 5,
            mcp_public_write_per_minute: 60,
            mcp_public_write_burst: 5,
            ..RateLimitSettings::default()
        }
    }

    fn router(settings: RateLimitSettings) -> axum::Router {
        router_with(RateLimiter::new(settings))
    }

    fn router_with(limiter: Arc<RateLimiter>) -> axum::Router {
        let public = axum::Router::new()
            .route("/auth/login", get(|| async { "login" }))
            .route("/auth/callback", get(|| async { "callback" }))
            .route(
                "/lines",
                post(|| async { "write" }).get(|| async { "read" }),
            );
        let private = axum::Router::new().route("/ingest", post(|| async { "ingest" }));
        axum::Router::new()
            .route("/Trips/plan", get(|| async { "plan" }))
            .route("/Train/by-uid/{uid}/{date}", get(|| async { "train" }))
            .nest("/public", public)
            .nest("/private", private)
            .layer(axum::middleware::from_fn_with_state(limiter, enforce))
    }

    async fn call(
        router: &axum::Router,
        method: &str,
        uri: &str,
        real_ip: Option<&str>,
        peer: &str,
    ) -> Response {
        let mut request = axum::http::Request::builder().method(method).uri(uri);
        if let Some(ip) = real_ip {
            request = request.header(REAL_IP_HEADER, ip);
        }
        let mut request = request.body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        router.clone().oneshot(request).await.unwrap()
    }

    const FRONTEND: &str = "10.42.0.9:40000";

    #[tokio::test]
    async fn hitting_the_limit_returns_429_with_retry_after() {
        let router = router(settings());
        for _ in 0..2 {
            let response = call(&router, "GET", "/Trips/plan", Some("203.0.113.5"), FRONTEND).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = call(&router, "GET", "/Trips/plan", Some("203.0.113.5"), FRONTEND).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after: u64 = response.headers()[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=2).contains(&retry_after), "{retry_after}");
    }

    #[tokio::test]
    async fn different_client_ips_have_independent_buckets() {
        let router = router(settings());
        for _ in 0..2 {
            call(
                &router,
                "GET",
                "/public/auth/login",
                Some("203.0.113.5"),
                FRONTEND,
            )
            .await;
        }
        let limited = call(
            &router,
            "GET",
            "/public/auth/login",
            Some("203.0.113.5"),
            FRONTEND,
        )
        .await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let other = call(
            &router,
            "GET",
            "/public/auth/login",
            Some("198.51.100.7"),
            FRONTEND,
        )
        .await;
        assert_eq!(other.status(), StatusCode::OK);
    }

    /// Each class has its own bucket.
    #[tokio::test]
    async fn classes_are_independent() {
        let router = router(settings());
        for _ in 0..3 {
            call(&router, "GET", "/Trips/plan", Some("203.0.113.5"), FRONTEND).await;
        }
        let response = call(
            &router,
            "GET",
            "/Train/by-uid/C12345/2026-09-27",
            Some("203.0.113.5"),
            FRONTEND,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn private_routes_are_never_limited() {
        let router = router(settings());
        for _ in 0..10 {
            let response = call(
                &router,
                "POST",
                "/private/ingest",
                Some("203.0.113.5"),
                FRONTEND,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn public_writes_are_limited_but_reads_are_not() {
        let router = router(settings());
        for _ in 0..10 {
            let response = call(
                &router,
                "GET",
                "/public/lines",
                Some("203.0.113.5"),
                FRONTEND,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        for _ in 0..2 {
            call(
                &router,
                "POST",
                "/public/lines",
                Some("203.0.113.5"),
                FRONTEND,
            )
            .await;
        }
        let response = call(
            &router,
            "POST",
            "/public/lines",
            Some("203.0.113.5"),
            FRONTEND,
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// Header trust on: different X-Real-IP values from the same frontend
    /// peer are different clients.
    #[tokio::test]
    async fn with_header_trust_on_the_header_is_the_key() {
        let router = router(settings());
        for ip in ["203.0.113.1", "203.0.113.2", "203.0.113.3", "203.0.113.4"] {
            let response = call(&router, "GET", "/Trips/plan", Some(ip), FRONTEND).await;
            assert_eq!(response.status(), StatusCode::OK, "{ip}");
        }
    }

    /// Header trust off: the header is ignored and the socket peer is the
    /// key, so a client can't dodge the limit by varying it.
    #[tokio::test]
    async fn with_header_trust_off_the_peer_is_the_key() {
        let router = router(RateLimitSettings {
            trust_x_real_ip: false,
            ..settings()
        });
        for ip in ["203.0.113.1", "203.0.113.2"] {
            let response = call(&router, "GET", "/Trips/plan", Some(ip), "192.0.2.10:5000").await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = call(
            &router,
            "GET",
            "/Trips/plan",
            Some("203.0.113.3"),
            "192.0.2.10:5000",
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let other_peer = call(&router, "GET", "/Trips/plan", None, "192.0.2.11:5000").await;
        assert_eq!(other_peer.status(), StatusCode::OK);
    }

    /// X-Forwarded-For is never a key: varying it changes nothing.
    #[tokio::test]
    async fn x_forwarded_for_is_ignored() {
        let router = router(settings());
        for n in 0..3 {
            let mut request = axum::http::Request::builder()
                .uri("/Trips/plan")
                .header("x-forwarded-for", format!("198.51.100.{n}"))
                .body(Body::empty())
                .unwrap();
            request
                .extensions_mut()
                .insert(ConnectInfo(FRONTEND.parse::<SocketAddr>().unwrap()));
            let response = router.clone().oneshot(request).await.unwrap();
            let expected = if n < 2 {
                StatusCode::OK
            } else {
                StatusCode::TOO_MANY_REQUESTS
            };
            assert_eq!(response.status(), expected, "request {n}");
        }
    }

    #[tokio::test]
    async fn disabled_limits_nothing() {
        let router = router(RateLimitSettings {
            enabled: false,
            ..settings()
        });
        for _ in 0..10 {
            let response = call(&router, "GET", "/Trips/plan", Some("203.0.113.5"), FRONTEND).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[test]
    fn the_bucket_refills_at_the_sustained_rate() {
        let limiter = RateLimiter::new(RateLimitSettings {
            trip_plan_per_minute: 60,
            trip_plan_burst: 1,
            ..settings()
        });
        let client = ClientKey::Ip("203.0.113.5".parse().unwrap());
        let start = Instant::now();
        assert!(
            limiter
                .check_at(LimitClass::TripPlan, client, start)
                .is_ok()
        );
        let retry = limiter
            .check_at(LimitClass::TripPlan, client, start)
            .expect_err("burst of 1 used up");
        assert_eq!(retry, Duration::from_secs(1));
        assert!(
            limiter
                .check_at(LimitClass::TripPlan, client, start + Duration::from_secs(1))
                .is_ok()
        );
    }

    #[test]
    fn ipv6_clients_are_keyed_by_their_slash_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::2".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(normalise(a), normalise(b));
        assert_ne!(normalise(a), normalise(c));
        let mapped: IpAddr = "::ffff:203.0.113.5".parse().unwrap();
        assert_eq!(normalise(mapped), "203.0.113.5".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn classification() {
        assert_eq!(
            classify(&Method::GET, "/public/auth/login"),
            Some(LimitClass::Login)
        );
        assert_eq!(
            classify(&Method::GET, "/public/auth/callback"),
            Some(LimitClass::Login)
        );
        assert_eq!(
            classify(&Method::GET, "/Trips/plan"),
            Some(LimitClass::TripPlan)
        );
        assert_eq!(
            classify(&Method::POST, "/Train/by-uid/C1/2026-09-27/track"),
            Some(LimitClass::TrainByUid)
        );
        assert_eq!(
            classify(&Method::DELETE, "/public/account"),
            Some(LimitClass::PublicWrite)
        );
        assert_eq!(classify(&Method::POST, "/private/train-events"), None);
        assert_eq!(classify(&Method::GET, "/private/stanox-crs"), None);
        assert_eq!(classify(&Method::GET, "/public/lines"), None);
        assert_eq!(
            classify(&Method::GET, "/Line/Mode/national-rail/Status"),
            None
        );
    }

    #[test]
    fn mcp_budget_defaults_are_five_times_public() {
        let d = RateLimitSettings::default();
        assert_eq!(
            (d.mcp_trip_plan_per_minute, d.mcp_trip_plan_burst),
            (100, 30)
        );
        assert_eq!(
            (d.mcp_train_by_uid_per_minute, d.mcp_train_by_uid_burst),
            (600, 200)
        );
        assert_eq!(
            (d.mcp_public_write_per_minute, d.mcp_public_write_burst),
            (600, 200)
        );
        assert_eq!(d.mcp_trip_plan_per_minute, 5 * d.trip_plan_per_minute);
        assert_eq!(d.mcp_train_by_uid_per_minute, 5 * d.train_by_uid_per_minute);
        assert_eq!(d.mcp_public_write_per_minute, 5 * d.public_write_per_minute);
    }

    #[test]
    fn a_zero_mcp_rate_is_rejected() {
        let settings = RateLimitSettings {
            mcp_trip_plan_burst: 0,
            ..RateLimitSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn service_keys_display_as_svc_colon_caller() {
        assert_eq!(ClientKey::Service(Caller::Mcp).to_string(), "svc:mcp");
        assert_eq!(ClientKey::Service(Caller::Mcp).caller(), Caller::Mcp);
        let ip = ClientKey::Ip("203.0.113.5".parse().unwrap());
        assert_eq!(ip.caller(), Caller::Public);
    }

    fn claims(groups: &[&str]) -> ServiceClaims {
        ServiceClaims {
            sub: "srv-ds-mcp".to_string(),
            iss: "https://sso.example/".to_string(),
            aud: crate::auth::internal_oauth::Audience::Single("internal".to_string()),
            exp: i64::MAX,
            nbf: None,
            iat: None,
            groups: groups.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn bearer_decision_grants_the_mcp_budget_only_to_a_valid_token_in_the_group() {
        assert_eq!(
            bearer_decision(&Ok(claims(&["other", "srv-ds-mcp"])), "srv-ds-mcp"),
            BearerDecision::Mcp
        );
        // A valid token for another service (wrong group): refused, not
        // anonymous and certainly not the MCP budget.
        assert_eq!(
            bearer_decision(&Ok(claims(&["svc-poller-tfl"])), "srv-ds-mcp"),
            BearerDecision::Reject(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            bearer_decision(&Ok(claims(&[])), "srv-ds-mcp"),
            BearerDecision::Reject(StatusCode::FORBIDDEN)
        );
        // An empty configured group never matches (defence in depth; the
        // limiter is inert then anyway).
        assert_eq!(
            bearer_decision(&Ok(claims(&[""])), ""),
            BearerDecision::Reject(StatusCode::FORBIDDEN)
        );
        for err in [
            VerifyError::Malformed,
            VerifyError::UnknownKey,
            VerifyError::Invalid,
        ] {
            assert_eq!(
                bearer_decision(&Err(err), "srv-ds-mcp"),
                BearerDecision::Reject(StatusCode::UNAUTHORIZED)
            );
        }
    }

    /// Never consulted: proves the inert and no-bearer paths don't verify.
    struct PanickingVerifier;

    impl ServiceTokenCheck for PanickingVerifier {
        fn verify_service_token<'a>(
            &'a self,
            _token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<ServiceClaims, VerifyError>> + Send + 'a>> {
            panic!("the verifier must not be called on this path")
        }
    }

    #[test]
    fn an_empty_mcp_group_makes_the_feature_inert() {
        assert!(ServiceCallerAuth::new(Arc::new(PanickingVerifier), "").is_none());
        assert!(ServiceCallerAuth::new(Arc::new(PanickingVerifier), "  ").is_none());
        assert!(ServiceCallerAuth::new(Arc::new(PanickingVerifier), "srv-ds-mcp").is_some());
    }

    /// No bearer: the anonymous path never touches the verifier.
    #[tokio::test]
    async fn without_a_bearer_the_verifier_is_never_called() {
        let router = router_with(RateLimiter::with_service_callers(
            settings(),
            ServiceCallerAuth::new(Arc::new(PanickingVerifier), "srv-ds-mcp"),
        ));
        for _ in 0..2 {
            let response = call(&router, "GET", "/Trips/plan", Some("203.0.113.5"), FRONTEND).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = call(&router, "GET", "/Trips/plan", Some("203.0.113.5"), FRONTEND).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // --- Integration-style: a real ServiceTokenVerifier against a mock
    // Authentik JWKS (the same test_support the /private suite uses). ---

    use crate::auth::internal_oauth::test_support::{mock_authentik, sign_token, valid_claims};

    /// The MCP pod calling in-cluster: no X-Real-IP, its own pod IP as peer.
    const MCP_POD: &str = "10.42.7.3:51000";

    async fn call_with_bearer(
        router: &axum::Router,
        method: &str,
        uri: &str,
        bearer: Option<&str>,
        peer: &str,
    ) -> Response {
        let mut request = axum::http::Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let mut request = request.body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        router.clone().oneshot(request).await.unwrap()
    }

    fn token(issuer: &str, groups: &[&str]) -> String {
        let groups: Vec<String> = groups.iter().map(ToString::to_string).collect();
        sign_token(&valid_claims(issuer, |c| {
            c["sub"] = serde_json::json!("srv-ds-mcp");
            c["groups"] = serde_json::json!(groups);
        }))
    }

    async fn mcp_router() -> (wiremock::MockServer, axum::Router) {
        let (server, verifier) = mock_authentik().await;
        let router = router_with(RateLimiter::with_service_callers(
            settings(),
            ServiceCallerAuth::new(Arc::new(verifier), "srv-ds-mcp"),
        ));
        (server, router)
    }

    /// A valid MCP token gets the MCP budget (burst 5 here, vs 2 public),
    /// keyed on the identity: changing the peer doesn't reset it, and it
    /// doesn't touch the anonymous bucket of the same peer.
    #[tokio::test]
    async fn a_valid_mcp_token_gets_its_own_finite_budget_keyed_on_identity() {
        let (server, router) = mcp_router().await;
        let mcp = token(&server.uri(), &["srv-ds-mcp"]);
        for n in 0..5 {
            let peer = if n % 2 == 0 {
                MCP_POD
            } else {
                "10.42.7.4:51000"
            };
            let response = call_with_bearer(&router, "GET", "/Trips/plan", Some(&mcp), peer).await;
            assert_eq!(response.status(), StatusCode::OK, "request {n}");
        }
        let limited =
            call_with_bearer(&router, "GET", "/Trips/plan", Some(&mcp), "10.42.9.9:1").await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(limited.headers().contains_key(header::RETRY_AFTER));

        // The same pod without a bearer is anonymous, with its own bucket.
        let anonymous = call_with_bearer(&router, "GET", "/Trips/plan", None, MCP_POD).await;
        assert_eq!(anonymous.status(), StatusCode::OK);

        // Other classes have their own MCP buckets.
        let train = call_with_bearer(
            &router,
            "GET",
            "/Train/by-uid/C12345/2026-09-28",
            Some(&mcp),
            MCP_POD,
        )
        .await;
        assert_eq!(train.status(), StatusCode::OK);
    }

    /// Another service's valid token (wrong group) is refused 403, and
    /// never charged to, or granted, any budget.
    #[tokio::test]
    async fn a_valid_token_without_the_mcp_group_is_forbidden() {
        let (server, router) = mcp_router().await;
        let tfl = token(&server.uri(), &["svc-poller-tfl"]);
        for _ in 0..4 {
            let response =
                call_with_bearer(&router, "GET", "/Trips/plan", Some(&tfl), MCP_POD).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }

    /// Expired, garbage and wrong-audience bearers are 401 with
    /// `WWW-Authenticate`, never a silent fall-back to the anonymous bucket.
    #[tokio::test]
    async fn an_invalid_bearer_is_unauthorized_not_anonymous() {
        let (server, router) = mcp_router().await;
        let expired = sign_token(&valid_claims(&server.uri(), |c| {
            c["groups"] = serde_json::json!(["srv-ds-mcp"]);
            c["exp"] =
                serde_json::json!((chrono::Utc::now() - chrono::Duration::hours(1)).timestamp());
        }));
        let wrong_audience = sign_token(&valid_claims(&server.uri(), |c| {
            c["groups"] = serde_json::json!(["srv-ds-mcp"]);
            c["aud"] = serde_json::json!("someone-else");
        }));
        for bad in [expired.as_str(), wrong_audience.as_str(), "not-a-jwt"] {
            for _ in 0..3 {
                let response =
                    call_with_bearer(&router, "POST", "/public/lines", Some(bad), MCP_POD).await;
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{bad}");
                assert_eq!(
                    response.headers()[header::WWW_AUTHENTICATE],
                    "Bearer error=\"invalid_token\""
                );
            }
        }
    }

    /// Login ignores the bearer entirely: anonymous per-IP limit.
    #[tokio::test]
    async fn login_ignores_the_bearer() {
        let (server, router) = mcp_router().await;
        let mcp = token(&server.uri(), &["srv-ds-mcp"]);
        for _ in 0..2 {
            let response =
                call_with_bearer(&router, "GET", "/public/auth/login", Some(&mcp), MCP_POD).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let limited =
            call_with_bearer(&router, "GET", "/public/auth/login", Some("junk"), MCP_POD).await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// Unlimited routes (public reads, /private) never look at the bearer.
    #[tokio::test]
    async fn unlimited_routes_ignore_the_bearer() {
        let (_server, router) = mcp_router().await;
        for uri in ["/public/lines"] {
            let response = call_with_bearer(&router, "GET", uri, Some("junk"), MCP_POD).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response =
            call_with_bearer(&router, "POST", "/private/ingest", Some("junk"), MCP_POD).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// With the feature off, even a valid MCP token is just anonymous.
    #[tokio::test]
    async fn when_inert_a_valid_mcp_token_is_anonymous() {
        let (server, _verifier) = mock_authentik().await;
        let router = router(settings());
        let mcp = token(&server.uri(), &["srv-ds-mcp"]);
        for _ in 0..2 {
            let response =
                call_with_bearer(&router, "GET", "/Trips/plan", Some(&mcp), MCP_POD).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let limited = call_with_bearer(&router, "GET", "/Trips/plan", Some(&mcp), MCP_POD).await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn a_zero_rate_is_rejected() {
        let settings = RateLimitSettings {
            login_per_minute: 0,
            ..RateLimitSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn defaults_match_the_declared_values() {
        use clap::Parser;
        let parsed =
            RateLimitSettings::try_parse_from(std::iter::empty::<String>()).expect("parse");
        assert_eq!(parsed, RateLimitSettings::default());
    }
}

/// Every env var the edge settings read must be set on the chart's api
/// container, or an operator's values silently do nothing.
#[cfg(test)]
mod chart_env_wiring_tests {
    use clap::CommandFactory;

    #[test]
    fn every_edge_and_rate_limit_env_var_is_set_on_the_charts_api_container() {
        let chart = common::manifest_dir!()
            .join("../../charts/distant-signal/templates/api-deployment.yaml");
        let template = std::fs::read_to_string(&chart).expect("read api-deployment.yaml");
        let mut declared: Vec<String> = Vec::new();
        for command in [
            super::RateLimitSettings::command(),
            crate::edge::EdgeSettings::command(),
        ] {
            declared.extend(
                command
                    .get_arguments()
                    .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
                    .map(str::to_string),
            );
        }
        declared.push(crate::data::config::STORED_GROUPS_EXTRA_ENV.to_string());
        declared.push("TRIP_PLAN_GRAPH_CACHE_DATES".to_string());
        declared.push("TRIP_PLAN_GRAPH_CACHE_MAX_AGE_SECS".to_string());
        assert!(declared.len() >= 16, "{declared:?}");
        let missing: Vec<&String> = declared
            .iter()
            .filter(|env| !template.contains(&format!("- name: {env}\n")))
            .collect();
        assert!(
            missing.is_empty(),
            "not set on the api container: {missing:?}"
        );
    }
}
