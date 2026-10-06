//! Request series registered at 0 when `api` starts.
//!
//! **Why.** `axum_prometheus` creates a `distant_signal_http_requests_total`
//! series on its first request, already at 1, and `increase()`/`rate()`
//! cannot see that first increment: a series needs a previous sample to
//! increase from. With `api` restarting every few hours, a rare route such
//! as `/Trips/plan` (a handful of calls between restarts) read as zero
//! traffic, and a route's first 500 after a restart never moved a 5xx ratio.
//! The same goes for `api_trip_plan_graph_cache_total`'s `hit`/`miss`.
//!
//! **What, and why this set.** Registering every route × method × status
//! code would be unbounded in status and several hundred series in routes,
//! each duration-histogram one carrying 14 more. So:
//!
//! - `distant_signal_http_requests_total` for [`KEY_PUBLIC_ROUTES`] (what
//!   the frontend's main pages and the MCP call) and every `/private`
//!   ingest route (the list `require_internal_oauth` authorises, so it
//!   cannot drift), each × its own method × [`PRE_REGISTERED_STATUSES`]
//!   (200, 500, 503): the success series rare-route queries count, and the
//!   two 5xx codes the 5xx-ratio alerts and the 503 contract
//!   (`crate::unavailable`) care about. About 40 routes × 3 = 120 series.
//! - `distant_signal_http_requests_duration_seconds` only for the key public
//!   routes' 200s: 16 × 14 = 224 series.
//! - `distant_signal_api_trip_plan_graph_cache_total{result}` for `hit` and
//!   `miss`.
//!
//! Any other route or status still appears on first use, as before. In
//! Prometheus the route label is `exported_endpoint` (the scrape's own
//! `endpoint` label wins the name; see docs/metrics.md).

use axum::http::Method;

/// The public routes whose request series are registered at 0: what the
/// MCP calls (`/Trips/plan` above all) and what the frontend's home, line,
/// station and train pages load. `route_templates_exist` checks every one
/// against the real router.
pub const KEY_PUBLIC_ROUTES: &[(Method, &str)] = &[
    (Method::GET, "/Trips/plan"),
    (Method::GET, "/Train/by-uid/{train_uid}/{date}"),
    (Method::GET, "/Line/Mode/{mode}/Status"),
    (Method::GET, "/Line/{ids}/Status"),
    (Method::GET, "/StopPoint/{crs}/Disruption"),
    (Method::GET, "/public/lines"),
    (Method::GET, "/public/lines/{id}"),
    (Method::GET, "/public/lines/{id}/trains"),
    (Method::GET, "/public/stations"),
    (Method::GET, "/public/stations/{crs}/departures"),
    (Method::GET, "/public/trains/search"),
    (Method::GET, "/public/trains/resolve"),
    (Method::GET, "/public/freshness"),
    (Method::GET, "/public/history-retention"),
    (Method::GET, "/public/incidents"),
    (Method::GET, "/public/auth/session"),
];

/// The status codes each pre-registered route gets a 0 series for.
pub const PRE_REGISTERED_STATUSES: &[&str] = &["200", "500", "503"];

/// `routes::trips`' graph-cache counter and its two `result` values.
pub const TRIP_PLAN_GRAPH_CACHE_METRIC: &str = "api_trip_plan_graph_cache_total";
pub const TRIP_PLAN_GRAPH_CACHE_RESULTS: &[&str] = &["hit", "miss"];

/// Every `(method, endpoint label)` whose request counter is registered:
/// the key public routes, then each private route under `/private`.
pub fn pre_registered_routes(
    private_routes: &[(&'static str, Method, Vec<String>)],
) -> Vec<(Method, String)> {
    KEY_PUBLIC_ROUTES
        .iter()
        .map(|(method, path)| (method.clone(), (*path).to_string()))
        .chain(
            private_routes
                .iter()
                .map(|(path, method, _)| (method.clone(), format!("/private{path}"))),
        )
        .collect()
}

/// Registers the series above at 0. Call after the `axum_prometheus`
/// recorder is installed (`build_pair`), so the names carry its prefix.
///
/// The label order is `axum_prometheus`'s own (method, status, endpoint),
/// so the layer's first real request increments these series rather than
/// creating a second one.
pub fn register(private_routes: &[(&'static str, Method, Vec<String>)]) {
    let total = axum_prometheus::utils::requests_total_name();
    let duration = axum_prometheus::utils::requests_duration_name();
    for (method, endpoint) in pre_registered_routes(private_routes) {
        for status in PRE_REGISTERED_STATUSES {
            let labels = [
                ("method", method.as_str().to_string()),
                ("status", (*status).to_string()),
                ("endpoint", endpoint.clone()),
            ];
            metrics::counter!(total, &labels).increment(0);
        }
    }
    for (method, endpoint) in KEY_PUBLIC_ROUTES {
        let labels = [
            ("method", method.as_str().to_string()),
            ("status", "200".to_string()),
            ("endpoint", (*endpoint).to_string()),
        ];
        // Registering the handle is enough: the exporter renders an empty
        // histogram as zero buckets.
        let _ = metrics::histogram!(duration, &labels);
    }
    for result in TRIP_PLAN_GRAPH_CACHE_RESULTS {
        metrics::counter!(
            common::metrics::metric_name(TRIP_PLAN_GRAPH_CACHE_METRIC),
            "result" => *result
        )
        .increment(0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::extract::MatchedPath;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;

    /// Needs a Tokio context (the placeholder pool is built lazily).
    fn private_routes() -> Vec<(&'static str, Method, Vec<String>)> {
        crate::test_support::inert_app(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://nobody@127.0.0.1:1/none")
                .unwrap(),
        )
        .internal_oauth_routes
        .clone()
    }

    fn render(f: impl FnOnce()) -> String {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new()
            .set_buckets(axum_prometheus::utils::SECONDS_DURATION_BUCKETS)
            .unwrap()
            .build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, f);
        handle.render()
    }

    #[tokio::test]
    async fn every_pre_registered_series_renders_at_zero() {
        let private = private_routes();
        let rendered = render(|| register(&private));
        let name = axum_prometheus::utils::requests_total_name();
        for (method, endpoint) in pre_registered_routes(&private) {
            for status in PRE_REGISTERED_STATUSES {
                let line = format!(
                    "{name}{{method=\"{method}\",status=\"{status}\",endpoint=\"{endpoint}\"}} 0"
                );
                assert!(rendered.contains(&line), "missing {line}");
            }
        }
        let duration = axum_prometheus::utils::requests_duration_name();
        assert!(
            rendered.contains(&format!(
                "{duration}_count{{method=\"GET\",status=\"200\",endpoint=\"/Trips/plan\"}} 0"
            )),
            "{rendered}"
        );
        for result in TRIP_PLAN_GRAPH_CACHE_RESULTS {
            assert!(
                rendered.contains(&format!(
                    "distant_signal_api_trip_plan_graph_cache_total{{result=\"{result}\"}} 0"
                )),
                "{rendered}"
            );
        }
    }

    /// The layer's own increment (same labels, same order) lands on the
    /// pre-registered series instead of creating a duplicate.
    #[test]
    fn the_first_real_request_increments_the_registered_series() {
        let rendered = render(|| {
            register(&[]);
            metrics::counter!(
                axum_prometheus::utils::requests_total_name(),
                &[
                    ("method", "GET".to_string()),
                    ("status", "200".to_string()),
                    ("endpoint", "/Trips/plan".to_string()),
                ]
            )
            .increment(1);
        });
        let lines: Vec<&str> = rendered
            .lines()
            .filter(|line| {
                line.starts_with(axum_prometheus::utils::requests_total_name())
                    && line.contains("status=\"200\",endpoint=\"/Trips/plan\"")
            })
            .collect();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].ends_with(" 1"), "{lines:?}");
    }

    /// The bounded set: a few hundred series, not routes × every status.
    #[tokio::test]
    async fn the_set_stays_bounded() {
        let routes = pre_registered_routes(&private_routes());
        assert!(routes.len() < 60, "{}", routes.len());
        assert!(routes.len() * PRE_REGISTERED_STATUSES.len() < 200);
    }

    /// Every pre-registered template is a real route of the server's router
    /// (composed as `main.rs` composes it): a renamed route would otherwise
    /// leave a dead 0 series behind. (The method is not checked here: a
    /// layer runs outside a route's method dispatch. The private methods come
    /// from the same list the auth middleware enforces.)
    #[tokio::test]
    async fn route_templates_exist() {
        let app = crate::test_support::inert_app(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://nobody@127.0.0.1:1/none")
                .unwrap(),
        );
        let private = app.internal_oauth_routes.clone();
        let seen = Arc::new(Mutex::new(None::<String>));
        let seen_in_layer = Arc::clone(&seen);
        let router: axum::Router = crate::app::Router::new()
            .merge(crate::routes::line_status::router())
            .merge(crate::routes::train::router())
            .merge(crate::routes::journeys::router())
            .merge(crate::routes::journey_templates::router())
            .merge(crate::routes::trips::router())
            .nest("/public", crate::routes::public_router())
            .nest("/private", crate::routes::private_router(app.clone()))
            // Outermost on every route: records the match and answers at
            // once, so no handler or auth check runs.
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, _next: axum::middleware::Next| {
                    let seen = Arc::clone(&seen_in_layer);
                    async move {
                        *seen.lock().unwrap() = request
                            .extensions()
                            .get::<MatchedPath>()
                            .map(|matched| matched.as_str().to_string());
                        StatusCode::NO_CONTENT
                    }
                },
            ))
            .with_state(app);
        for (method, template) in pre_registered_routes(&private) {
            let concrete = template.replace('{', "x").replace('}', "");
            *seen.lock().unwrap() = None;
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(&concrete)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NO_CONTENT,
                "{method} {template} is not a route"
            );
            assert_eq!(
                seen.lock().unwrap().as_deref(),
                Some(template.as_str()),
                "{method} {concrete}"
            );
        }
    }

    /// `routes::trips` names the cache counter and its results inline; keep
    /// them in step with the constants registered here.
    #[test]
    fn the_graph_cache_counter_matches_routes_trips() {
        let source = include_str!("routes/trips.rs");
        assert!(source.contains(&format!("\"{TRIP_PLAN_GRAPH_CACHE_METRIC}\"")));
        for result in TRIP_PLAN_GRAPH_CACHE_RESULTS {
            assert!(
                source.contains(&format!("=> \"{result}\"")),
                "routes/trips.rs no longer records result {result}"
            );
        }
    }
}
