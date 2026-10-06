//! 503 + `Retry-After` when a dependency (almost always Postgres) is
//! unavailable, instead of a generic 500.
//!
//! **Why.** From 10:17 to 16:10 UTC on 2026-10-01 the Postgres pod was
//! stuck in `ImagePullBackOff`. Every route's `internal_error` helper
//! answered 500, so nothing calling `api` -- the ingest producers, the
//! frontend, the MCP -- could tell "Distant Signal is down for a while,
//! retry later" from "this request hit a bug". The MCP is about to depend
//! on `/Trips/plan` entirely, so that difference is now part of the
//! contract (docs/api-changelog.md, 2026-10-06).
//!
//! **How.** Every route's error helper asks [`response_for`] first. When
//! the error chain shows the database (or Redis, or an upstream HTTP
//! dependency) could not be reached, it answers
//! `503 {"error":"service_unavailable","retryable":true,...}` and marks the
//! request; [`annotate_unavailable`], one middleware over the whole router,
//! then adds `Content-Type: application/json` and `Retry-After` (from
//! `API_UNAVAILABLE_RETRY_AFTER_SECS`, see `crate::edge::EdgeSettings`).
//! Any other 503 (the trip planner's "too many plans" shed, the schedule
//! publish statement timeout) also gets a `Retry-After` if it has none,
//! since every 503 this service answers is retryable. Every other error
//! stays a 500.
//!
//! The mark travels in a task-local rather than through each handler's
//! `(StatusCode, String)` error type, so no route signature changes: a
//! handler runs inside the middleware's task, and an error mapped outside
//! one (no task-local in scope) still gets the 503 status and body, just
//! not the JSON content type.

use std::cell::Cell;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

/// The body of every dependency-unavailable 503. Stable: clients (the MCP)
/// match on `error` and `retryable`, never on `message`.
pub const UNAVAILABLE_BODY: &str = concat!(
    r#"{"error":"service_unavailable","retryable":true,"#,
    r#""message":"Distant Signal is temporarily unavailable. Please retry shortly."}"#
);

tokio::task_local! {
    /// Set by [`response_for`] when it answers 503; read by
    /// [`annotate_unavailable`].
    static MARKED_UNAVAILABLE: Cell<bool>;
}

/// SQLSTATEs that mean "the server is going away or not accepting
/// connections", not "this query is wrong":
///
/// - class 08, connection exception (08000, 08003 connection does not
///   exist, 08006 connection failure, 08001/08004 cannot establish);
/// - 57P01 `admin_shutdown`, 57P02 `crash_shutdown`, 57P03
///   `cannot_connect_now` (starting up or in recovery);
/// - 53300 `too_many_connections`.
///
/// 57014 (`query_canceled`, a statement timeout) is deliberately absent: it
/// is load or a slow query, and the schedule publish route already maps its
/// own case.
pub fn sqlstate_is_unavailable(code: &str) -> bool {
    code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03" | "53300")
}

/// Whether one sqlx error means the database could not be reached.
///
/// | variant | why |
/// |---|---|
/// | `PoolTimedOut` | no connection within the acquire timeout: the server is down or saturated |
/// | `PoolClosed` | the pool is shutting down |
/// | `Io` | connection refused/reset/aborted, a broken pipe, a DNS failure while connecting |
/// | `Tls` | the TLS handshake on connect failed |
/// | `Protocol` | the server sent something unexpected, in practice a connection cut mid-handshake or mid-query |
/// | `WorkerCrashed` | sqlx's connection worker died |
/// | `Database` | only for [`sqlstate_is_unavailable`] codes |
///
/// Everything else (`RowNotFound`, decode errors, constraint violations,
/// statement timeouts, ...) is a 500.
pub fn sqlx_error_is_unavailable(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::WorkerCrashed => true,
        sqlx::Error::Database(db) => db.code().is_some_and(|code| sqlstate_is_unavailable(&code)),
        _ => false,
    }
}

/// An I/O error kind that means a peer could not be reached. Narrower than
/// "any I/O error": a bare `NotFound` or `UnexpectedEof` from reading a
/// file is a bug, not an outage.
fn io_error_is_unavailable(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::TimedOut
    )
}

/// Whether one error in a chain means a dependency could not be reached.
pub fn cause_is_unavailable(cause: &(dyn std::error::Error + 'static)) -> bool {
    if let Some(err) = cause.downcast_ref::<sqlx::Error>() {
        return sqlx_error_is_unavailable(err);
    }
    if let Some(err) = cause.downcast_ref::<std::io::Error>() {
        return io_error_is_unavailable(err);
    }
    if let Some(err) = cause.downcast_ref::<redis::RedisError>() {
        return err.is_connection_refusal()
            || err.is_connection_dropped()
            || err.is_timeout()
            || err.is_io_error();
    }
    if let Some(err) = cause.downcast_ref::<reqwest::Error>() {
        return err.is_connect() || err.is_timeout();
    }
    false
}

/// Whether anything in `err`'s chain means a dependency is unavailable.
pub fn is_dependency_unavailable(err: &anyhow::Error) -> bool {
    err.chain().any(cause_is_unavailable)
}

/// The 503 for `err` if it means a dependency is unavailable, else `None`
/// (the caller logs and answers its usual 500). Logs at warn: during an
/// outage this fires on every request, and `DistantSignalApiDatabaseDown`
/// is what pages.
pub fn response_for(err: &anyhow::Error) -> Option<(StatusCode, String)> {
    if !is_dependency_unavailable(err) {
        return None;
    }
    tracing::warn!(error = ?err, "a dependency is unavailable; answering 503");
    // Outside the middleware (a unit test calling a handler directly) there
    // is nothing to mark; the status and body are still right.
    let _ = MARKED_UNAVAILABLE.try_with(|marked| marked.set(true));
    Some((StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE_BODY.to_string()))
}

/// [`response_for`], or a logged 500 with `message` as its body: the whole
/// of a typical route's error helper.
pub fn or_internal_error(
    err: &anyhow::Error,
    message: impl FnOnce() -> String,
) -> (StatusCode, String) {
    if let Some(unavailable) = response_for(err) {
        return unavailable;
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message())
}

/// The `Retry-After` value [`annotate_unavailable`] adds, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryAfter(pub Duration);

/// Middleware over the whole router: runs the request with the
/// unavailable mark in scope, then gives a marked 503 its JSON content type
/// and every 503 a `Retry-After` (an existing one is kept).
pub async fn annotate_unavailable(
    State(retry_after): State<RetryAfter>,
    request: Request,
    next: Next,
) -> Response {
    let (mut response, marked) = MARKED_UNAVAILABLE
        .scope(Cell::new(false), async move {
            let response = next.run(request).await;
            (response, MARKED_UNAVAILABLE.with(Cell::get))
        })
        .await;
    if response.status() == StatusCode::SERVICE_UNAVAILABLE {
        let headers = response.headers_mut();
        if marked {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
        }
        headers
            .entry(header::RETRY_AFTER)
            .or_insert_with(|| HeaderValue::from(retry_after.0.as_secs().max(1)));
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::routing::get;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    fn io(kind: std::io::ErrorKind) -> std::io::Error {
        std::io::Error::new(kind, "test")
    }

    #[test]
    fn connection_level_sqlx_errors_are_unavailable() {
        for err in [
            sqlx::Error::PoolTimedOut,
            sqlx::Error::PoolClosed,
            sqlx::Error::WorkerCrashed,
            sqlx::Error::Io(io(std::io::ErrorKind::ConnectionRefused)),
            sqlx::Error::Io(io(std::io::ErrorKind::ConnectionReset)),
            sqlx::Error::Io(io(std::io::ErrorKind::UnexpectedEof)),
            sqlx::Error::Protocol("unexpected message on connect".to_string()),
            sqlx::Error::Tls("handshake failed".into()),
        ] {
            assert!(sqlx_error_is_unavailable(&err), "{err:?}");
            assert!(
                is_dependency_unavailable(&anyhow::Error::from(err).context("load the lines")),
                "a context layer must not hide it"
            );
        }
    }

    #[test]
    fn query_level_sqlx_errors_stay_500() {
        for err in [
            sqlx::Error::RowNotFound,
            sqlx::Error::ColumnNotFound("crs".to_string()),
            sqlx::Error::Configuration("bad url".into()),
            sqlx::Error::Decode("bad json".into()),
        ] {
            assert!(!sqlx_error_is_unavailable(&err), "{err:?}");
        }
    }

    #[test]
    fn sqlstates() {
        for code in ["08000", "08001", "08003", "08004", "08006", "57P01", "57P02", "57P03", "53300"] {
            assert!(sqlstate_is_unavailable(code), "{code}");
        }
        // statement timeout, unique violation, serialization failure,
        // deadlock, undefined table, read-only transaction.
        for code in ["57014", "23505", "40001", "40P01", "42P01", "25006"] {
            assert!(!sqlstate_is_unavailable(code), "{code}");
        }
    }

    #[test]
    fn only_network_io_kinds_are_unavailable_on_their_own() {
        for kind in [
            std::io::ErrorKind::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::TimedOut,
        ] {
            assert!(is_dependency_unavailable(&io(kind).into()), "{kind:?}");
        }
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::UnexpectedEof,
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::InvalidData,
        ] {
            assert!(!is_dependency_unavailable(&io(kind).into()), "{kind:?}");
        }
    }

    #[test]
    fn redis_connection_errors_are_unavailable() {
        let refused = redis::RedisError::from(io(std::io::ErrorKind::ConnectionRefused));
        assert!(is_dependency_unavailable(&refused.into()));
        let type_error = redis::RedisError::from((redis::ErrorKind::TypeError, "not a string"));
        assert!(!is_dependency_unavailable(&type_error.into()));
    }

    #[test]
    fn an_unrelated_error_stays_500() {
        let err = anyhow::anyhow!("no such line");
        assert!(response_for(&err).is_none());
        assert_eq!(
            or_internal_error(&err, || "query failed".to_string()),
            (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
        );
    }

    #[test]
    fn the_body_is_stable_json() {
        let body: serde_json::Value = serde_json::from_str(UNAVAILABLE_BODY).unwrap();
        assert_eq!(body["error"], "service_unavailable");
        assert_eq!(body["retryable"], true);
        assert!(body["message"].is_string());
    }

    const RETRY_AFTER: RetryAfter = RetryAfter(Duration::from_secs(30));

    async fn call(router: axum::Router, uri: &str) -> Response {
        router
            .layer(axum::middleware::from_fn_with_state(
                RETRY_AFTER,
                annotate_unavailable,
            ))
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn assert_unavailable_headers(response: &Response) {
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "30");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/json"
        );
    }

    /// A real route over a pool whose server refuses connections (port 1):
    /// the acquire fails, the route's own `internal_error` helper answers
    /// 503, the middleware adds the headers.
    #[tokio::test]
    async fn an_unreachable_database_is_a_503_with_retry_after() {
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(500))
            .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/none")
            .unwrap();
        let router = crate::routes::departures::router()
            .with_state(crate::test_support::inert_app(pool));
        let response = call(router, "/stations/ZQT/departures").await;
        assert_unavailable_headers(&response);
        let body: serde_json::Value = serde_json::from_str(&body_of(response).await).unwrap();
        assert_eq!(body["error"], "service_unavailable");
        assert_eq!(body["retryable"], true);
    }

    #[tokio::test]
    async fn a_closed_pool_is_a_503_with_retry_after() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/none")
            .unwrap();
        pool.close().await;
        let router = crate::routes::departures::router()
            .with_state(crate::test_support::inert_app(pool));
        let response = call(router, "/stations/ZQT/departures").await;
        assert_unavailable_headers(&response);
        assert_eq!(body_of(response).await, UNAVAILABLE_BODY);
    }

    #[tokio::test]
    async fn any_other_503_gets_retry_after_but_keeps_its_own_body_and_header() {
        let router = axum::Router::new()
            .route(
                "/busy",
                get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "too many plans") }),
            )
            .route(
                "/own",
                get(|| async {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        [(header::RETRY_AFTER, "5")],
                        "slow down",
                    )
                }),
            );
        let busy = call(router.clone(), "/busy").await;
        assert_eq!(busy.headers()[header::RETRY_AFTER], "30");
        assert!(busy.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/plain"));
        assert_eq!(body_of(busy).await, "too many plans");
        let own = call(router, "/own").await;
        assert_eq!(own.headers()[header::RETRY_AFTER], "5");
    }

    #[tokio::test]
    async fn a_500_is_left_alone() {
        let router = axum::Router::new().route(
            "/bug",
            get(|| async { or_internal_error(&anyhow::anyhow!("bug"), || "query failed".into()) }),
        );
        let response = call(router, "/bug").await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response.headers().get(header::RETRY_AFTER).is_none());
    }
}
