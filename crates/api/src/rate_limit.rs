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
//! `distant_signal_api_rate_limited_total{class}`.
//!
//! The algorithm is GCRA (a token bucket kept as one timestamp per key):
//! `per_minute` sustained, `burst` at once. State is per pod, in memory.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

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
        Ok(())
    }

    fn quota(&self, class: LimitClass) -> Quota {
        let (per_minute, burst) = match class {
            LimitClass::Login => (self.login_per_minute, self.login_burst),
            LimitClass::TripPlan => (self.trip_plan_per_minute, self.trip_plan_burst),
            LimitClass::TrainByUid => (self.train_by_uid_per_minute, self.train_by_uid_burst),
            LimitClass::PublicWrite => (self.public_write_per_minute, self.public_write_burst),
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

/// The limiter: one GCRA timestamp per (class, client key).
pub struct RateLimiter {
    settings: RateLimitSettings,
    state: Mutex<LimiterState>,
    warned_about_peer_fallback: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct LimiterState {
    /// Theoretical arrival time per key: the bucket is empty until then.
    tat: HashMap<(LimitClass, IpAddr), Instant>,
    checks: u64,
}

/// Where a request's key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    RealIpHeader,
    Peer,
}

impl RateLimiter {
    pub fn new(settings: RateLimitSettings) -> Arc<Self> {
        Arc::new(Self {
            settings,
            state: Mutex::new(LimiterState::default()),
            warned_about_peer_fallback: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// `Ok` if a request by `client` in `class` is allowed at `now` (and
    /// records it), else `Err(retry_after)`.
    fn check_at(&self, class: LimitClass, client: IpAddr, now: Instant) -> Result<(), Duration> {
        let quota = self.settings.quota(class);
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
            return Err(ahead - quota.tolerance);
        }
        state.tat.insert(key, new_tat);
        Ok(())
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
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let bits = u128::from(v6) & !((1u128 << 64) - 1);
                IpAddr::V6(Ipv6Addr::from(bits))
            }
        },
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
    match limiter.check_at(class, client, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            metrics::counter!(
                common::metrics::metric_name("api_rate_limited_total"),
                "class" => class.label()
            )
            .increment(1);
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
            ..RateLimitSettings::default()
        }
    }

    fn router(settings: RateLimitSettings) -> axum::Router {
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
            .layer(axum::middleware::from_fn_with_state(
                RateLimiter::new(settings),
                enforce,
            ))
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
        let client: IpAddr = "203.0.113.5".parse().unwrap();
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
        let chart = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
