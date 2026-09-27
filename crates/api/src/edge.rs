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
}

impl Default for EdgeSettings {
    fn default() -> Self {
        Self {
            request_timeout_secs: 30,
            private_request_timeout_secs: 300,
            header_read_timeout_secs: 10,
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
        ] {
            ensure!(value > 0, "{name} must be at least 1 second");
        }
        Ok(())
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
}

/// Interval between HTTP/2 keep-alive pings, and how long to wait for the
/// answer before closing the connection.
const HTTP2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);
const HTTP2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Serves `router` on `listener` until the process exits: `axum::serve`'s
/// accept loop, plus the HTTP/1 header-read timeout and HTTP/2 keep-alive
/// it doesn't expose. Each request carries the peer's
/// `axum::extract::ConnectInfo<SocketAddr>`, as
/// `into_make_service_with_connect_info` would give it.
pub async fn serve(
    mut listener: tokio::net::TcpListener,
    router: axum::Router,
    header_read_timeout: Duration,
) -> std::io::Result<()> {
    loop {
        // `axum::serve::Listener::accept` retries transient accept errors
        // (EMFILE and friends) with a backoff instead of returning them.
        let (stream, remote_addr) = axum::serve::Listener::accept(&mut listener).await;
        let router = router.clone();
        tokio::spawn(serve_connection(
            stream,
            remote_addr,
            router,
            header_read_timeout,
        ));
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    remote_addr: SocketAddr,
    router: axum::Router,
    header_read_timeout: Duration,
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

    if let Err(err) = builder
        .serve_connection_with_upgrades(TokioIo::new(stream), service)
        .await
    {
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
    fn a_zero_timeout_is_rejected() {
        let settings = EdgeSettings {
            request_timeout_secs: 0,
            ..EdgeSettings::default()
        };
        assert!(settings.validate().is_err());
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
