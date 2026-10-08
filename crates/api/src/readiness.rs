//! `GET /public/ready`: the api's readiness endpoint (2026-10-08).
//!
//! `/public/health` answers 200 whenever the process is listening; it stays
//! the liveness (and startup) probe, so a database blip never restarts api
//! pods. Readiness is this endpoint instead, which answers 503 while
//!
//! - the pod is shutting down ([`crate::shutdown::ShutdownSignal`]), so an
//!   endpoint controller that has not yet seen the pod terminating takes it
//!   out on the next probe; and, only with `API_READINESS_CHECKS_DB=true`,
//! - the request pool cannot run `SELECT 1` within [`PROBE_TIMEOUT`].
//!
//! The database check is OFF by default (user decision, 2026-10-08): with a
//! single api replica, `NotReady` empties the Service, so a short database
//! blip or pool contention would become a full outage, and callers would
//! get connection errors instead of the api's 503 + `Retry-After`. It is
//! for two or more replicas, where one pod that cannot reach the database
//! can be taken out while the others serve.
//!
//! The database check is bounded ([`PROBE_TIMEOUT`], under the chart's 3 s
//! probe timeout) and cached for [`CACHE_TTL`] behind one async mutex, so
//! however many callers probe at once (the endpoint is under `/public`,
//! reachable through the frontend) a pod runs at most one `SELECT 1` per
//! [`CACHE_TTL`], and concurrent callers wait for that one probe instead of
//! starting their own.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sqlx::PgPool;

use crate::shutdown::ShutdownSignal;

/// How long one database check may take. Below the chart's readiness
/// `timeoutSeconds` (3), so a slow database gives a 503, not a probe
/// timeout.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a database check's result is reused. Half the chart's
/// readiness period (10 s), so each kubelet probe normally sees a fresh
/// result while a probe storm still costs one query per 5 s.
pub const CACHE_TTL: Duration = Duration::from_secs(5);

/// Whether readiness also checks the database (default false; see the
/// module doc).
pub const CHECK_DATABASE_ENV: &str = "API_READINESS_CHECKS_DB";

/// [`CHECK_DATABASE_ENV`] from the environment: unset or empty is false;
/// otherwise `true` or `false` in any case. Anything else is an error, so a
/// typo cannot silently change the readiness semantics.
pub fn check_database_from_env() -> anyhow::Result<bool> {
    parse_check_database(std::env::var(CHECK_DATABASE_ENV).ok().as_deref())
}

fn parse_check_database(value: Option<&str>) -> anyhow::Result<bool> {
    match value.map(str::trim) {
        None | Some("") => Ok(false),
        Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
        Some(v) => anyhow::bail!("{CHECK_DATABASE_ENV} must be true or false, got {v:?}"),
    }
}

/// What `/public/ready` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState {
    Ready,
    DatabaseUnreachable,
    Draining,
}

impl ReadyState {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::DatabaseUnreachable => "database_unreachable",
            Self::Draining => "draining",
        }
    }
}

/// The readiness check's state: the pool, the shutdown flag, whether the
/// database is checked, and the cached result of the last database check.
#[derive(Debug)]
pub struct Readiness {
    pool: PgPool,
    shutdown: ShutdownSignal,
    check_database: bool,
    probe_timeout: Duration,
    cache_ttl: Duration,
    last: tokio::sync::Mutex<Option<(Instant, bool)>>,
}

impl Readiness {
    pub fn new(pool: PgPool, shutdown: ShutdownSignal, check_database: bool) -> Arc<Self> {
        Self::with_timings(pool, shutdown, check_database, PROBE_TIMEOUT, CACHE_TTL)
    }

    pub fn with_timings(
        pool: PgPool,
        shutdown: ShutdownSignal,
        check_database: bool,
        probe_timeout: Duration,
        cache_ttl: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            pool,
            shutdown,
            check_database,
            probe_timeout,
            cache_ttl,
            last: tokio::sync::Mutex::new(None),
        })
    }

    /// The current state. Draining wins without touching the database; with
    /// the database check off, anything else is ready.
    pub async fn check(&self) -> ReadyState {
        if self.shutdown.is_triggered() {
            return ReadyState::Draining;
        }
        if !self.check_database {
            return ReadyState::Ready;
        }
        // Held across the probe: concurrent callers queue here and then
        // read the result the first one stored.
        let mut last = self.last.lock().await;
        let db_up = match *last {
            Some((at, up)) if at.elapsed() < self.cache_ttl => up,
            _ => {
                let outcome = ds_store::pool::probe(&self.pool, self.probe_timeout).await;
                if let Err(err) = &outcome {
                    tracing::warn!(error = %err, "readiness: the database check failed");
                }
                let up = outcome.is_ok();
                *last = Some((Instant::now(), up));
                up
            }
        };
        drop(last);
        // A shutdown that began during the probe still wins.
        if self.shutdown.is_triggered() {
            ReadyState::Draining
        } else if db_up {
            ReadyState::Ready
        } else {
            ReadyState::DatabaseUnreachable
        }
    }
}

/// `/ready`, with its own state; merge it under `/public`. The returned
/// router takes any outer state, so it merges into [`crate::app::Router`].
pub fn router<S: Clone + Send + Sync + 'static>(readiness: Arc<Readiness>) -> axum::Router<S> {
    axum::Router::new()
        .route("/ready", get(get_ready))
        .with_state(readiness)
}

async fn get_ready(State(readiness): State<Arc<Readiness>>) -> Response {
    let state = readiness.check().await;
    let status = if state == ReadyState::Ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let mut response =
        (status, Json(serde_json::json!({ "status": state.label() }))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    /// A pool whose every connection attempt is refused (nothing listens on
    /// port 1), failing fast.
    fn unreachable_pool() -> PgPool {
        PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(500))
            .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/none")
            .expect("lazy pool")
    }

    async fn get_status(readiness: Arc<Readiness>) -> (StatusCode, String) {
        let response = router::<()>(readiness)
            .oneshot(Request::get("/ready").body(Body::empty()).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// Default (check off, the single-replica setting): a database that
    /// cannot be reached does not make the pod `NotReady`.
    #[tokio::test]
    async fn with_the_check_off_an_unreachable_database_is_still_ready() {
        let readiness = Readiness::new(unreachable_pool(), ShutdownSignal::new(), false);
        let (status, body) =
            tokio::time::timeout(Duration::from_millis(200), get_status(readiness))
                .await
                .expect("no database round trip with the check off");
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("\"ready\""), "{body}");
    }

    #[tokio::test]
    async fn with_the_check_on_an_unreachable_database_is_503() {
        let readiness = Readiness::new(unreachable_pool(), ShutdownSignal::new(), true);
        let (status, body) = get_status(readiness).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("database_unreachable"), "{body}");
    }

    /// Draining is 503 in both modes, without a database round trip.
    #[tokio::test]
    async fn draining_is_503_in_both_modes_without_a_database_check() {
        for check_database in [false, true] {
            let shutdown = ShutdownSignal::new();
            // Were the database checked, this would wait out the unreachable
            // pool's 0.5 s acquire timeout; draining short-circuits instead.
            let readiness = Readiness::with_timings(
                unreachable_pool(),
                shutdown.clone(),
                check_database,
                Duration::from_secs(30),
                CACHE_TTL,
            );
            shutdown.trigger();
            let (status, body) =
                tokio::time::timeout(Duration::from_millis(200), get_status(readiness))
                    .await
                    .expect("no database round trip while draining");
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{check_database}");
            assert!(body.contains("draining"), "{check_database}: {body}");
        }
    }

    #[test]
    fn the_check_setting_defaults_to_false_and_rejects_typos() {
        assert!(!parse_check_database(None).expect("unset"));
        assert!(!parse_check_database(Some("")).expect("empty"));
        assert!(!parse_check_database(Some("FALSE")).expect("false"));
        assert!(parse_check_database(Some(" True ")).expect("true"));
        assert!(parse_check_database(Some("yes")).is_err());
    }

    /// The cached result is reused: with the cache warm, a check returns
    /// at once even though a real probe of this pool would take its full
    /// acquire timeout.
    #[tokio::test]
    async fn the_result_is_cached() {
        let readiness = Readiness::with_timings(
            unreachable_pool(),
            ShutdownSignal::new(),
            true,
            PROBE_TIMEOUT,
            Duration::from_secs(60),
        );
        assert_eq!(readiness.check().await, ReadyState::DatabaseUnreachable);
        let started = Instant::now();
        for _ in 0..20 {
            assert_eq!(readiness.check().await, ReadyState::DatabaseUnreachable);
        }
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                readiness -- --ignored --test-threads=1`"]
    async fn with_the_check_on_a_reachable_database_is_ready_then_draining_flips_it() {
        let shutdown = ShutdownSignal::new();
        let readiness = Readiness::new(connect().await, shutdown.clone(), true);
        let (status, body) = get_status(Arc::clone(&readiness)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("\"ready\""), "{body}");
        shutdown.trigger();
        let (status, body) = get_status(readiness).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("draining"), "{body}");
    }
}
