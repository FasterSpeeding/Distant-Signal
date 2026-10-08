//! The api's HTTP edge: the listener loop and its time limits (API-2).
//!
//! `axum::serve` sets no header-read, body-read or request timeout, so a
//! client that trickles its headers or body a byte at a time could hold a
//! connection (and, once inside a handler, a database connection or a
//! trip-planning slot) for as long as it liked. The api Service is
//! reachable from every pod in the cluster and, through the frontend proxy,
//! from the internet. Three limits close that:
//!
//! - an HTTP/1 header-read timeout on the connection itself ([`serve`]),
//!   which is the only way to bound a slow-headers client, since no tower
//!   layer runs until the headers have arrived;
//! - a whole-request [`tower_http::timeout::TimeoutLayer`] per router
//!   ([`EdgeSettings::public_timeout_layer`] and
//!   [`EdgeSettings::private_timeout_layer`]), which bounds body reads too:
//!   every extractor that reads the body runs inside the handler future the
//!   layer times;
//! - HTTP/2 keep-alive pings, so a dead HTTP/2 peer's connection is closed.
//!
//! On shutdown ([`serve_with_shutdown`]) the listener stops accepting, every
//! open connection is told to finish (HTTP/1 closes after its in-flight
//! response, keep-alive or not; HTTP/2 gets a GOAWAY), and the server waits
//! up to `API_SHUTDOWN_DRAIN_SECS` for them.
//!
//! These are read from the environment with their own parser rather than
//! from `data::config::ServiceArguments`, so adding one doesn't touch every
//! test that builds a `ServiceArguments` by hand.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use axum::http::StatusCode;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tower::ServiceExt;
use tower_http::timeout::TimeoutLayer;

/// Settings for the api's HTTP edge, all from the environment.
#[derive(Debug, Clone, PartialEq, Eq, clap::Parser)]
#[command(name = "api-edge", no_binary_name = true)]
pub struct EdgeSettings {
    /// Whole-request time limit for every public route, body read included.
    /// A request still running at this point gets a 408. The slowest
    /// legitimate public request is a cold `/Trips/plan` graph build (a few
    /// seconds) or a ticket upload (its parser has its own 10 s limit).
    #[arg(long, env = "API_REQUEST_TIMEOUT_SECS", default_value_t = 30)]
    pub request_timeout_secs: u64,

    /// Whole-request time limit for the internal `/private/*` ingest
    /// routes. Longer than the public one: they take up to 100 MB bodies,
    /// and the final schedule publish chunk runs statements with a 120 s
    /// timeout, which its caller waits up to 180 s for.
    #[arg(long, env = "API_PRIVATE_REQUEST_TIMEOUT_SECS", default_value_t = 300)]
    pub private_request_timeout_secs: u64,

    /// How long an HTTP/1 client has to send its complete request headers
    /// before the connection is closed.
    #[arg(long, env = "API_HEADER_READ_TIMEOUT_SECS", default_value_t = 10)]
    pub header_read_timeout_secs: u64,

    /// The `Retry-After` (seconds) on a 503: a route that could not reach
    /// the database answers 503 with this, see `crate::unavailable`. Long
    /// enough that a client retrying on it doesn't add load to a recovering
    /// database, short enough that a pod restart is retried promptly.
    #[arg(long, env = "API_UNAVAILABLE_RETRY_AFTER_SECS", default_value_t = 30)]
    pub unavailable_retry_after_secs: u64,

    /// On SIGTERM, how long in-flight requests get to finish (and the
    /// background loops to stop and release their locks) before the process
    /// exits anyway. Must fit inside the pod's terminationGracePeriodSeconds
    /// after the `preStop` sleep; the chart checks that. A request still
    /// running at the deadline is cut off, so a `/private` publish chunk
    /// longer than this is retried by its producer as before.
    #[arg(long, env = "API_SHUTDOWN_DRAIN_SECS", default_value_t = 20)]
    pub shutdown_drain_secs: u64,
}

impl Default for EdgeSettings {
    fn default() -> Self {
        Self {
            request_timeout_secs: 30,
            private_request_timeout_secs: 300,
            header_read_timeout_secs: 10,
            unavailable_retry_after_secs: 30,
            shutdown_drain_secs: 20,
        }
    }
}

impl EdgeSettings {
    /// Reads the settings from the environment (no command-line flags: the
    /// server's argv belongs to `ServiceArguments`). Zero is rejected for
    /// every limit, since a zero timeout would fail every request.
    pub fn from_env() -> Result<Self> {
        use clap::Parser;
        let settings = Self::try_parse_from(std::iter::empty::<String>())
            .context("invalid api edge settings in the environment")?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("API_REQUEST_TIMEOUT_SECS", self.request_timeout_secs),
            (
                "API_PRIVATE_REQUEST_TIMEOUT_SECS",
                self.private_request_timeout_secs,
            ),
            (
                "API_HEADER_READ_TIMEOUT_SECS",
                self.header_read_timeout_secs,
            ),
            (
                "API_UNAVAILABLE_RETRY_AFTER_SECS",
                self.unavailable_retry_after_secs,
            ),
            ("API_SHUTDOWN_DRAIN_SECS", self.shutdown_drain_secs),
        ] {
            ensure!(value > 0, "{name} must be at least 1 second");
        }
        ensure!(
            self.unavailable_retry_after_secs <= 3600,
            "API_UNAVAILABLE_RETRY_AFTER_SECS must be at most 3600 seconds"
        );
        ensure!(
            self.shutdown_drain_secs <= 3600,
            "API_SHUTDOWN_DRAIN_SECS must be at most 3600 seconds"
        );
        Ok(())
    }

    pub fn shutdown_drain(&self) -> Duration {
        Duration::from_secs(self.shutdown_drain_secs)
    }

    pub fn public_timeout_layer(&self) -> TimeoutLayer {
        TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(self.request_timeout_secs),
        )
    }

    pub fn private_timeout_layer(&self) -> TimeoutLayer {
        TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(self.private_request_timeout_secs),
        )
    }

    pub fn header_read_timeout(&self) -> Duration {
        Duration::from_secs(self.header_read_timeout_secs)
    }

    pub fn unavailable_retry_after(&self) -> crate::unavailable::RetryAfter {
        crate::unavailable::RetryAfter(Duration::from_secs(self.unavailable_retry_after_secs))
    }
}

/// Interval between HTTP/2 keep-alive pings, and how long to wait for the
/// answer before closing the connection.
const HTTP2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);
const HTTP2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Serves `router` on `listener` until the process exits; see
/// [`serve_with_shutdown`].
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: axum::Router,
    header_read_timeout: Duration,
) -> std::io::Result<()> {
    serve_with_shutdown(
        listener,
        router,
        header_read_timeout,
        std::future::pending(),
        Duration::MAX,
    )
    .await
    .map(drop)
}

/// How a [`serve_with_shutdown`] drain ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// Every connection finished within the deadline.
    Drained,
    /// The deadline passed with connections still open; they are dropped
    /// when the process exits.
    DeadlineReached,
}

/// Serves `router` on `listener` until `shutdown` resolves: `axum::serve`'s
/// accept loop, plus the HTTP/1 header-read timeout and HTTP/2 keep-alive
/// it doesn't expose. Each request carries the peer's
/// `axum::extract::ConnectInfo<SocketAddr>`, as
/// `into_make_service_with_connect_info` would give it.
///
/// Once `shutdown` resolves the listener is closed (a new connection is
/// refused), each open connection is shut down gracefully (an in-flight
/// request finishes and gets its response; an idle keep-alive connection is
/// closed; HTTP/2 gets a GOAWAY), and this returns when they have all
/// ended or after `drain_deadline`, whichever is first.
pub async fn serve_with_shutdown(
    mut listener: tokio::net::TcpListener,
    router: axum::Router,
    header_read_timeout: Duration,
    shutdown: impl Future<Output = ()>,
    drain_deadline: Duration,
) -> std::io::Result<DrainOutcome> {
    let graceful = hyper_util::server::graceful::GracefulShutdown::new();
    tokio::pin!(shutdown);
    loop {
        let (stream, remote_addr) = tokio::select! {
            // `axum::serve::Listener::accept` retries transient accept
            // errors (EMFILE and friends) with a backoff instead of
            // returning them. Cancel-safe: dropping it mid-wait loses no
            // connection.
            accepted = axum::serve::Listener::accept(&mut listener) => accepted,
            () = &mut shutdown => break,
        };
        tokio::spawn(serve_connection(
            stream,
            remote_addr,
            router.clone(),
            header_read_timeout,
            graceful.watcher(),
        ));
    }
    // Close the listening socket first, so the kernel refuses new
    // connections instead of queueing them on a backlog nobody accepts.
    drop(listener);
    let open = graceful.count();
    tracing::info!(
        open_connections = open,
        drain_deadline_secs = drain_deadline.as_secs(),
        "shutting down: no longer accepting connections; draining the open ones"
    );
    let outcome = if tokio::time::timeout(drain_deadline, graceful.shutdown())
        .await
        .is_ok()
    {
        tracing::info!("every connection drained");
        DrainOutcome::Drained
    } else {
        tracing::warn!(
            drain_deadline_secs = drain_deadline.as_secs(),
            "drain deadline reached with connections still open; exiting anyway"
        );
        DrainOutcome::DeadlineReached
    };
    Ok(outcome)
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    remote_addr: SocketAddr,
    router: axum::Router,
    header_read_timeout: Duration,
    watcher: hyper_util::server::graceful::Watcher,
) {
    let service =
        hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
            let mut request = request.map(axum::body::Body::new);
            request
                .extensions_mut()
                .insert(axum::extract::ConnectInfo(remote_addr));
            router.clone().oneshot(request)
        });

    let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(header_read_timeout);
    builder
        .http2()
        .timer(TokioTimer::new())
        .keep_alive_interval(HTTP2_KEEP_ALIVE_INTERVAL)
        .keep_alive_timeout(HTTP2_KEEP_ALIVE_TIMEOUT)
        // Same as axum::serve: needed for HTTP/2 websockets.
        .enable_connect_protocol();

    let connection = builder
        .serve_connection_with_upgrades(TokioIo::new(stream), service)
        .into_owned();
    if let Err(err) = watcher.watch(connection).await {
        tracing::trace!(error = %err, %remote_addr, "connection ended with an error");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::routing::{get, post};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn defaults_match_the_documented_values() {
        use clap::Parser;
        let parsed = EdgeSettings::try_parse_from(std::iter::empty::<String>()).expect("parse");
        // The environment of a test run doesn't set these; if it ever does,
        // this is the test to look at.
        assert_eq!(parsed, EdgeSettings::default());
    }

    #[test]
    fn an_out_of_range_drain_is_rejected() {
        for secs in [0, 3601] {
            let settings = EdgeSettings {
                shutdown_drain_secs: secs,
                ..EdgeSettings::default()
            };
            assert!(settings.validate().is_err(), "{secs}");
        }
    }

    #[test]
    fn a_zero_timeout_is_rejected() {
        let settings = EdgeSettings {
            request_timeout_secs: 0,
            ..EdgeSettings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn an_out_of_range_retry_after_is_rejected() {
        for secs in [0, 3601] {
            let settings = EdgeSettings {
                unavailable_retry_after_secs: secs,
                ..EdgeSettings::default()
            };
            assert!(settings.validate().is_err(), "{secs}");
        }
    }

    async fn spawn(router: axum::Router, header_read_timeout: Duration) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(serve(listener, router, header_read_timeout));
        addr
    }

    /// API-2: a client that never finishes its headers is disconnected.
    #[tokio::test]
    async fn a_client_that_never_finishes_its_headers_is_disconnected() {
        let router = axum::Router::new().route("/", get(|| async { "ok" }));
        let addr = spawn(router, Duration::from_millis(200)).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
            .await
            .expect("write partial headers");
        let mut buf = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf))
            .await
            .expect("the server must close the connection, not wait forever");
        // Closed (EOF or reset), possibly after a 408.
        let _ = read;
        let text = String::from_utf8_lossy(&buf);
        assert!(
            buf.is_empty() || text.starts_with("HTTP/1.1 408"),
            "unexpected response: {text}"
        );
    }

    /// A normal request still works, and the handler sees the peer address.
    #[tokio::test]
    async fn a_normal_request_is_served_with_connect_info() {
        let router = axum::Router::new().route(
            "/",
            get(
                |axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>| async move {
                    peer.ip().to_string()
                },
            ),
        );
        let addr = spawn(router, Duration::from_secs(5)).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .expect("write");
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.expect("read");
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.ends_with("127.0.0.1"), "{text}");
    }

    async fn read_all(stream: &mut tokio::net::TcpStream) -> String {
        let mut buf = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf))
            .await
            .expect("the server must close the connection")
            .expect("read");
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Graceful shutdown: once the signal fires, a request already in
    /// flight still gets its full response, an idle keep-alive connection
    /// is closed, a new connection is refused, and the server returns once
    /// the in-flight request is done.
    #[tokio::test]
    async fn shutdown_drains_in_flight_requests_and_refuses_new_connections() {
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let router = axum::Router::new()
            .route("/fast", get(|| async { "fast" }))
            .route(
                "/slow",
                get({
                    let entered = std::sync::Arc::clone(&entered);
                    let release = std::sync::Arc::clone(&release);
                    move || async move {
                        entered.notify_one();
                        release.notified().await;
                        "slow done"
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let shutdown = crate::shutdown::ShutdownSignal::new();
        let server = tokio::spawn(serve_with_shutdown(
            listener,
            router,
            Duration::from_secs(5),
            shutdown.triggered(),
            Duration::from_secs(10),
        ));

        // An idle keep-alive connection: one request served, left open.
        let mut idle = tokio::net::TcpStream::connect(addr).await.expect("connect");
        idle.write_all(b"GET /fast HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .expect("write");
        let mut buf = vec![0u8; 512];
        let n = idle.read(&mut buf).await.expect("read");
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));

        // An in-flight request (keep-alive, too), parked in its handler.
        let mut in_flight = tokio::net::TcpStream::connect(addr).await.expect("connect");
        in_flight
            .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .expect("write");
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .expect("the slow handler starts");

        shutdown.trigger();

        // New connections are refused once the listener is closed.
        let refused = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match tokio::net::TcpStream::connect(addr).await {
                    Err(err) => break err,
                    Ok(_) => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        })
        .await
        .expect("new connections must be refused after the shutdown signal");
        // A connect racing the close can be reset instead (it landed on the
        // backlog the kernel then dropped); once closed, it is refused.
        let _ = refused;
        let after = tokio::net::TcpStream::connect(addr)
            .await
            .expect_err("the listener is closed");
        assert_eq!(after.kind(), std::io::ErrorKind::ConnectionRefused);

        // The idle keep-alive connection is closed without a response.
        assert_eq!(read_all(&mut idle).await, "");

        // Still draining: the slow request holds the server open.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !server.is_finished(),
            "returned before the in-flight request finished"
        );

        release.notify_one();
        let response = read_all(&mut in_flight).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("slow done"), "{response}");

        let outcome = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the server returns once drained")
            .expect("task")
            .expect("serve");
        assert_eq!(outcome, DrainOutcome::Drained);
    }

    /// A request that outlives the drain deadline does not hold the
    /// process: the server returns at the deadline.
    #[tokio::test]
    async fn shutdown_gives_up_at_the_drain_deadline() {
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let router = axum::Router::new().route(
            "/stuck",
            get({
                let entered = std::sync::Arc::clone(&entered);
                move || async move {
                    entered.notify_one();
                    std::future::pending::<()>().await;
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let shutdown = crate::shutdown::ShutdownSignal::new();
        let server = tokio::spawn(serve_with_shutdown(
            listener,
            router,
            Duration::from_secs(5),
            shutdown.triggered(),
            Duration::from_millis(200),
        ));
        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(b"GET /stuck HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .expect("write");
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .expect("the handler starts");
        shutdown.trigger();
        let outcome = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the server returns at the deadline")
            .expect("task")
            .expect("serve");
        assert_eq!(outcome, DrainOutcome::DeadlineReached);
    }

    /// API-2: a body that trickles in slower than the request timeout gets a
    /// 408 instead of holding the handler open.
    #[tokio::test]
    async fn a_slow_body_hits_the_request_timeout() {
        let settings = EdgeSettings {
            request_timeout_secs: 1,
            ..EdgeSettings::default()
        };
        let router = axum::Router::new()
            .route("/", post(|body: String| async move { body }))
            .layer(settings.public_timeout_layer());
        let addr = spawn(router, Duration::from_secs(5)).await;

        let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\nabc")
            .await
            .expect("write");
        let mut buf = vec![0u8; 256];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("a response must arrive once the request timeout passes")
            .expect("read");
        let text = String::from_utf8_lossy(&buf[..n]);
        assert!(text.starts_with("HTTP/1.1 408"), "{text}");
    }
}
