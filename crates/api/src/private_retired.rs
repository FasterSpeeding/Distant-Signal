//! `API_PRIVATE_ROUTES=false`: the `/private` ingest routes retired behind a
//! counted 404 (ingest phase 5, step 5.1; docs/ingest-phase5-runbook.md).
//!
//! **Default on.** Unset, empty or `true` keeps every `/private/*` route
//! exactly as before. `false` is the phase 5 soak: once every producer runs
//! on its `db`/`stream` sink, Ranma sets `api.privateRoutes.enabled: false`
//! for 7 days before the ingest code is deleted (5.2).
//!
//! **Why a fallback, not just a skipped nest.** Without a `/private` nest a
//! straggler's request becomes one more `/{unmatched}` 404 and the soak can
//! no longer tell which producer still calls which route (decided, Q4: a
//! counted 404, not 410). So `/private` keeps a nest whose only content is a
//! fallback that:
//!
//! - answers `404 {"error":"private_routes_retired"}`;
//! - increments `distant_signal_api_private_route_retired_total{route,method}`.
//!   `route` is the `/private/...` path of a pair from the api's own route
//!   table (`AppState::internal_oauth_routes`, the list
//!   `require_internal_oauth` enforced), `method` its method; anything else
//!   is `route="other", method="other"`. So the label set is bounded: one
//!   series per pair plus `other`, all registered at 0 when this mode is on
//!   ([`register_metrics`]);
//! - logs the caller's verified `sub` when the request carries an internal
//!   OAuth bearer the api can still verify, so a straggler names itself.
//!
//! The chart's `DistantSignalApiPrivateRouteRetiredCalled` alert fires on any
//! increase, and renders only while `api.privateRoutes.enabled` is false.

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;

use crate::app::{App, Router};

/// The env var, chart `api.privateRoutes.enabled`.
pub const PRIVATE_ROUTES_ENV: &str = "API_PRIVATE_ROUTES";

/// The counter's name after `common::metrics::metric_name`'s prefix:
/// `distant_signal_api_private_route_retired_total`.
pub const PRIVATE_ROUTE_RETIRED_METRIC: &str = "api_private_route_retired_total";

/// The `route` and `method` label for a request that matches no pair.
pub const OTHER: &str = "other";

/// [`PRIVATE_ROUTES_ENV`] from the environment: unset or empty is `true`
/// (the routes stay); otherwise `true` or `false` in any case. Anything else
/// is an error, so a typo cannot silently retire (or keep) the routes.
/// Logs the choice when the routes are retired.
pub fn private_routes_from_env() -> anyhow::Result<bool> {
    let private_routes = parse_private_routes(std::env::var(PRIVATE_ROUTES_ENV).ok().as_deref())?;
    if !private_routes {
        tracing::warn!(
            "{PRIVATE_ROUTES_ENV}=false: every /private/* request answers a counted 404 \
             (distant_signal_{PRIVATE_ROUTE_RETIRED_METRIC})"
        );
    }
    Ok(private_routes)
}

fn parse_private_routes(value: Option<&str>) -> anyhow::Result<bool> {
    match value.map(str::trim) {
        None | Some("") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
        Some(v) => anyhow::bail!("{PRIVATE_ROUTES_ENV} must be true or false, got {v:?}"),
    }
}

/// The `(route, method)` labels for a request to `path` (as seen inside the
/// `/private` nest, so without the prefix) with `method`: the pair's own
/// when the table has exactly that path and method, else [`OTHER`] for both.
pub fn retired_labels(
    routes: &[(&'static str, Method, Vec<String>)],
    path: &str,
    method: &Method,
) -> (String, String) {
    routes
        .iter()
        .find(|(route, route_method, _)| *route == path && route_method == method)
        .map_or_else(
            || (OTHER.to_string(), OTHER.to_string()),
            |(route, route_method, _)| (format!("/private{route}"), route_method.to_string()),
        )
}

/// Every `(route, method)` label pair the counter can carry: each table pair,
/// then `other`.
pub fn label_set(routes: &[(&'static str, Method, Vec<String>)]) -> Vec<(String, String)> {
    routes
        .iter()
        .map(|(route, method, _)| (format!("/private{route}"), method.to_string()))
        .chain(std::iter::once((OTHER.to_string(), OTHER.to_string())))
        .collect()
}

/// Registers every [`label_set`] series at 0, so the alert's `increase()`
/// sees a straggler's first call. Only when the routes are retired.
pub fn register_metrics(routes: &[(&'static str, Method, Vec<String>)]) {
    let name = common::metrics::metric_name(PRIVATE_ROUTE_RETIRED_METRIC);
    for (route, method) in label_set(routes) {
        metrics::counter!(name.clone(), "route" => route, "method" => method).increment(0);
    }
}

fn record(route: String, method: String) {
    metrics::counter!(
        common::metrics::metric_name(PRIVATE_ROUTE_RETIRED_METRIC),
        "route" => route,
        "method" => method
    )
    .increment(1);
}

/// The router nested at `/private` while the routes are retired: a fallback
/// only, so every path and method under `/private` lands in [`retired`].
pub fn router() -> Router {
    Router::new().fallback(retired)
}

async fn retired(State(app): State<App>, request: Request) -> impl IntoResponse {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let (route, method_label) = retired_labels(&app.internal_oauth_routes, &path, &method);
    // Only a bearer the api verifies names a caller; an unverifiable one is
    // logged as such, never its unverified claims.
    let caller = match crate::auth::bearer_token(request.headers()) {
        None => "none".to_string(),
        Some(token) => match app.internal_oauth_verifier.verify(&token).await {
            Ok(claims) => claims.sub,
            Err(_) => "unverified bearer".to_string(),
        },
    };
    tracing::warn!(
        route,
        method = %method,
        caller,
        "call to a retired /private route ({PRIVATE_ROUTES_ENV}=false): answered 404. A \
         producer still on its HTTP sink? See docs/ingest-phase5-runbook.md step 5.1"
    );
    record(route, method_label);
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "private_routes_retired"})),
    )
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use axum_prometheus::metrics_exporter_prometheus::PrometheusBuilder;
    use tower::ServiceExt;

    use super::*;

    fn app() -> App {
        crate::test_support::inert_app(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://nobody@127.0.0.1:1/none")
                .unwrap(),
        )
    }

    /// The server's routes as `main.rs` composes them, with `/private`
    /// either on or retired.
    fn server(app: &App, private_routes: bool) -> axum::Router {
        Router::new()
            .merge(crate::routes::line_status::router())
            .merge(crate::routes::train::router())
            .nest("/public", crate::routes::public_router())
            .nest(
                "/private",
                crate::routes::private_or_retired_router(app.clone(), private_routes),
            )
            .with_state(app.clone())
    }

    async fn status(router: &axum::Router, method: Method, uri: &str) -> StatusCode {
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    #[test]
    fn the_env_defaults_to_on_and_rejects_typos() {
        assert!(parse_private_routes(None).unwrap());
        assert!(parse_private_routes(Some("")).unwrap());
        assert!(parse_private_routes(Some(" TRUE ")).unwrap());
        assert!(!parse_private_routes(Some("false")).unwrap());
        assert!(!parse_private_routes(Some("False")).unwrap());
        assert!(parse_private_routes(Some("off")).is_err());
        assert!(parse_private_routes(Some("0")).is_err());
    }

    #[tokio::test]
    async fn labels_come_from_the_route_table_and_stay_bounded() {
        let app = app();
        let routes = &app.internal_oauth_routes;
        assert_eq!(
            retired_labels(routes, "/stanox-crs", &Method::GET),
            ("/private/stanox-crs".to_string(), "GET".to_string())
        );
        assert_eq!(
            retired_labels(routes, "/train-events", &Method::POST),
            ("/private/train-events".to_string(), "POST".to_string())
        );
        // A known path with a method it never had, a sub-path, an unknown
        // path: all `other`.
        for (path, method) in [
            ("/train-events", Method::DELETE),
            ("/stanox-crs/extra", Method::GET),
            ("/wp-login.php", Method::GET),
            ("/", Method::GET),
        ] {
            assert_eq!(
                retired_labels(routes, path, &method),
                (OTHER.to_string(), OTHER.to_string()),
                "{method} {path}"
            );
        }
        let labels = label_set(routes);
        assert_eq!(labels.len(), routes.len() + 1);
        assert!(labels.len() < 60, "{}", labels.len());
    }

    #[tokio::test]
    async fn every_series_is_registered_at_zero() {
        let app = app();
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, || {
            register_metrics(&app.internal_oauth_routes);
        });
        let rendered = handle.render();
        for (route, method) in label_set(&app.internal_oauth_routes) {
            let line = format!(
                "distant_signal_api_private_route_retired_total{{route=\"{route}\",method=\"{method}\"}} 0"
            );
            assert!(rendered.contains(&line), "missing {line} in {rendered}");
        }
    }

    /// Off: every `/private` request is a 404, counted under its pair (or
    /// `other`), and the public routes still answer.
    #[test]
    fn retired_routes_answer_a_counted_404() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let app = app();
                    let router = server(&app, false);
                    for (method, uri) in [
                        (Method::GET, "/private/stanox-crs"),
                        (Method::GET, "/private/stanox-crs"),
                        (Method::POST, "/private/train-events"),
                        (Method::GET, "/private/nope"),
                        (Method::GET, "/private"),
                    ] {
                        assert_eq!(
                            status(&router, method.clone(), uri).await,
                            StatusCode::NOT_FOUND,
                            "{method} {uri}"
                        );
                    }
                    assert_eq!(
                        status(&router, Method::GET, "/public/health").await,
                        StatusCode::OK
                    );
                });
        });
        let rendered = handle.render();
        for line in [
            r#"distant_signal_api_private_route_retired_total{route="/private/stanox-crs",method="GET"} 2"#,
            r#"distant_signal_api_private_route_retired_total{route="/private/train-events",method="POST"} 1"#,
            r#"distant_signal_api_private_route_retired_total{route="other",method="other"} 2"#,
        ] {
            assert!(rendered.contains(line), "missing {line} in {rendered}");
        }
    }

    /// On (the default): `/private` is the ingest router as before (its auth
    /// middleware answers 401 to a request without a bearer), nothing is
    /// counted, and the public routes answer the same.
    #[test]
    fn enabled_routes_are_unchanged() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let app = app();
                    let router = server(&app, true);
                    assert_eq!(
                        status(&router, Method::GET, "/private/stanox-crs").await,
                        StatusCode::UNAUTHORIZED
                    );
                    assert_eq!(
                        status(&router, Method::POST, "/private/train-events").await,
                        StatusCode::UNAUTHORIZED
                    );
                    assert_eq!(
                        status(&router, Method::GET, "/public/health").await,
                        StatusCode::OK
                    );
                });
        });
        assert!(
            !handle.render().contains("private_route_retired"),
            "{}",
            handle.render()
        );
    }
}
