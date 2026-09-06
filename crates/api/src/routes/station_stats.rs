//! `GET /public/stations/{crs}/sample-stats`: per-(station, operator)
//! delay/cancellation stats, computed on demand from `station_samples`.
//! Unauthenticated, same `public_router()` pattern as `reference.rs`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::station_stats::compute_station_operator_stats;
use crate::render::{full_coverage_availability_json, sample_availability_json, sample_stats_json};

pub fn router() -> Router {
    Router::new().route(
        "/stations/{crs}/sample-stats",
        axum::routing::get(get_station_sample_stats),
    )
}

/// 404s only when BOTH `station_samples` and `station_full_coverage_samples`
/// have no row for `crs` at all -- widened per design doc Decision 5, since
/// full coverage's route-membership gate (`full_coverage_enabled_for`) is
/// structurally wider than LDBWS's `sample_stations`-only reach: a station
/// can have a real full-coverage row for an operator with zero LDBWS
/// coverage. Mirrors `get_stop_point_disruption`'s existing "not covered"
/// honesty precedent (`crates/api/src/routes/line_status.rs:278-295`).
/// `200 []` is a different, equally real fact: a row exists but genuinely
/// has zero departures/rows right now (a quiet board).
async fn get_station_sample_stats(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let sample = queries::latest_station_sample(&app.database, &crs)
        .await
        .map_err(internal_error)?;
    let full_coverage_rows = queries::latest_station_full_coverage_samples(&app.database, &crs)
        .await
        .map_err(internal_error)?;

    if sample.is_none() && full_coverage_rows.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no sample data collected for station: {crs}"),
        ));
    }

    let custom = crate::data::custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    let mut lines: Vec<common::LineDefinition> = app.config.lines.to_vec();
    lines.extend(custom.into_iter().map(common::LineDefinition::from));

    // No per-station override mechanism exists (Decision 3) --
    // `Defaults::default()` is the same baseline `aggregator` uses for
    // any line without its own `severity_overrides`.
    let defaults = common::Defaults::default();
    let empty_sample = || common::StationSample {
        crs: crs.clone(),
        polled_at: chrono::Utc::now(),
        departures: vec![],
    };
    let stats = compute_station_operator_stats(
        &sample.unwrap_or_else(empty_sample),
        &defaults,
        &full_coverage_rows,
        &lines,
        app.config.full_coverage_enabled_default,
    );

    Ok(Json(
        stats
            .into_iter()
            .map(|s| {
                let mut out = json!({
                    "operator": s.operator,
                    "sampleAvailability": sample_availability_json(&s.availability),
                });
                if let Some(stats) = s.availability.sample_stats() {
                    out["sampleStats"] = sample_stats_json(&stats);
                }
                if let Some(stats) = &s.full_coverage_stats {
                    out["fullCoverageStats"] = sample_stats_json(stats);
                }
                out["fullCoverageAvailability"] =
                    full_coverage_availability_json(&s.full_coverage_availability);
                out
            })
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "station sample-stats query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn route_path_parses_and_dispatches() {
        // Cheap insurance the exact path-segment syntax is right -- mirrors
        // `routes::line_status`'s own module-doc-documented precedent for a
        // similarly shaped concern (a throwaway probe router, not the real
        // app state).
        async fn probe(Path(crs): Path<String>) -> String {
            crs
        }

        let app: axum::Router =
            axum::Router::new().route("/stations/{crs}/sample-stats", axum::routing::get(probe));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/stations/EDB/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(String::from_utf8(body.to_vec()).unwrap(), "EDB");
    }
}

/// HTTP-layer tests exercised against a live database -- mirrors
/// `crates/api/src/data/custom_lines.rs`'s `db_tests` seed/assert/delete
/// pattern, and `line_status.rs::db_tests`'s `test_app` helper for
/// constructing a real `App` around a live pool without needing every
/// other part of `AppState::init` (OIDC discovery, etc). Uses the reserved
/// `Z…` fixture CRS namespace (same convention as
/// docs/superpowers/plans/2026-09-02-frontend-ux-review-fixes.md Task 1)
/// so cleanup can never touch real data.
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Copied from `routes::line_status::db_tests::test_app` (that
    /// module's own doc comment: colocated per-file rather than shared,
    /// until a third file needs it too). Every field an inert placeholder
    /// except `database`, which the caller supplies -- this route touches
    /// nothing else on `App`.
    fn test_app(pool: PgPool) -> App {
        test_app_with_lines(pool, vec![])
    }

    /// Like `test_app`, but with a real static line catalogue -- needed by
    /// the full-coverage gating tests below. `custom_lines::CustomLine` has
    /// no `full_coverage_enabled` field at all (`From<CustomLine> for
    /// LineDefinition` hardcodes it `false` -- a user-defined line is never
    /// a full-coverage rollout candidate, per that impl's own doc comment),
    /// so a custom-line fixture structurally cannot exercise
    /// `full_coverage_enabled_for` returning `true`. This deviates from
    /// the plan's own sketch (which suggested seeding a custom line for
    /// this), corrected here to use the mechanism the gate actually reads:
    /// `app.config.lines`, the static catalogue.
    fn test_app_with_lines(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
        test_app_with_lines_and_full_coverage_default(pool, lines, false)
    }

    /// Like `test_app_with_lines`, but with an explicit
    /// `full_coverage_enabled_default` -- needed by the global-override
    /// gating test below.
    fn test_app_with_lines_and_full_coverage_default(
        pool: PgPool,
        lines: Vec<common::LineDefinition>,
        full_coverage_enabled_default: bool,
    ) -> App {
        let config = ServiceArguments {
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
            defaults_file: None,
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default,
            schedule_match_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
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
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_fixture(pool: &PgPool, crs: &str) {
        sqlx::query("DELETE FROM station_samples WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .expect("cleanup fixture station_samples row");
    }

    async fn delete_full_coverage_fixture(pool: &PgPool, crs: &str, operator: &str) {
        sqlx::query("DELETE FROM station_full_coverage_samples WHERE crs = $1 AND operator = $2")
            .bind(crs)
            .bind(operator)
            .execute(pool)
            .await
            .expect("cleanup fixture station_full_coverage_samples row");
    }

    /// Minimal `LineDefinition` fixture -- only `id`/`operators`/`stations`/
    /// `full_coverage_enabled` need real values for these tests, matching
    /// how `crates/api/src/data/station_stats.rs`'s own tests construct
    /// minimal fixtures.
    fn gating_line(
        crs: &str,
        operator: &str,
        full_coverage_enabled: bool,
    ) -> common::LineDefinition {
        common::LineDefinition {
            id: "ztest-line".to_string(),
            name: "Z Test Line".to_string(),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: vec![operator.to_string()],
            stations: vec![common::Station {
                crs: crs.to_string(),
                tiploc: None,
                role: "minor".to_string(),
                segment: None,
            }],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_no_row_for_crs_is_404_naming_the_crs() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQS").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQS/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("ZQS"), "404 body should name the CRS: {body}");

        delete_fixture(&pool, "ZQS").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_a_row_present_with_empty_departures_is_200_empty_array() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQQ").await;

        sqlx::query(
            "INSERT INTO station_samples (crs, polled_at, departures) VALUES ('ZQQ', NOW(), '[]')",
        )
        .execute(&pool)
        .await
        .expect("seed empty-departures fixture row");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQQ/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!([]));

        delete_fixture(&pool, "ZQQ").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_two_operators_render_alphabetically_with_correct_wire_shape() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQR").await;

        // ZA clears Defaults::default().min_sample_size (3) with three
        // departures, one delayed past the 5-minute threshold and one
        // skipping this specific station -- exercises the `Available` wire
        // shape end to end, including nested `sampleStats.avgDelayMinutes`
        // camelCase. ZB has only one departure -- stays `BelowThreshold`.
        let departures = serde_json::json!([
            {
                "service_id": "svc-1", "operator": "ZA", "destination_crs": "WAT",
                "scheduled": "10:00", "estimated": "10:07", "is_cancelled": false,
                "delay_minutes": 7, "skipped_stations": []
            },
            {
                "service_id": "svc-2", "operator": "ZA", "destination_crs": "WAT",
                "scheduled": "10:10", "estimated": "On time", "is_cancelled": false,
                "delay_minutes": 0, "skipped_stations": ["ZQR"]
            },
            {
                "service_id": "svc-3", "operator": "ZA", "destination_crs": "WAT",
                "scheduled": "10:20", "estimated": "On time", "is_cancelled": false,
                "delay_minutes": 0, "skipped_stations": []
            },
            {
                "service_id": "svc-4", "operator": "ZB", "destination_crs": "WAT",
                "scheduled": "10:30", "estimated": "10:30", "is_cancelled": false,
                "delay_minutes": 0, "skipped_stations": []
            }
        ]);
        sqlx::query(
            "INSERT INTO station_samples (crs, polled_at, departures) VALUES ('ZQR', NOW(), $1)",
        )
        .bind(departures)
        .execute(&pool)
        .await
        .expect("seed two-operator fixture row");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQR/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.as_array().unwrap().len(), 2);
        // Alphabetical order: ZA before ZB.
        assert_eq!(json[0]["operator"], "ZA");
        assert_eq!(
            json[0]["sampleAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert_eq!(
            json[0]["sampleStats"],
            serde_json::json!({
                "total": 3, "delayed": 1, "cancelled": 0, "skipped": 1, "avgDelayMinutes": 7.0 / 3.0
            })
        );

        assert_eq!(json[1]["operator"], "ZB");
        assert_eq!(
            json[1]["sampleAvailability"],
            serde_json::json!({"state": "below-threshold", "observed": 1, "required": 3})
        );
        assert!(json[1].get("sampleStats").is_none());

        delete_fixture(&pool, "ZQR").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_full_coverage_row_with_no_station_samples_row_is_200_not_404() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQF").await;
        delete_full_coverage_fixture(&pool, "ZQF", "ZF").await;

        sqlx::query(
            "INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats) \
             VALUES ('ZQF', 'ZF', NOW(), \
             '{\"total\":40,\"delayed\":4,\"cancelled\":1,\"skipped\":0,\"avg_delay_minutes\":2.5}')",
        )
        .execute(&pool)
        .await
        .expect("seed full-coverage-only fixture row");

        let router: axum::Router =
            crate::app::Router::new()
                .merge(router())
                .with_state(test_app_with_lines(
                    pool.clone(),
                    vec![gating_line("ZQF", "ZF", true)],
                ));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQF/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["operator"], "ZF");
        assert_eq!(
            json[0]["sampleAvailability"],
            serde_json::json!({"state": "below-threshold", "observed": 0, "required": 3})
        );
        assert_eq!(
            json[0]["fullCoverageAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert_eq!(
            json[0]["fullCoverageStats"],
            serde_json::json!({
                "total": 40, "delayed": 4, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 2.5
            })
        );

        delete_fixture(&pool, "ZQF").await;
        delete_full_coverage_fixture(&pool, "ZQF", "ZF").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_full_coverage_row_present_but_gate_disabled_is_not_enabled() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQG").await;
        delete_full_coverage_fixture(&pool, "ZQG", "ZG").await;

        sqlx::query(
            "INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats) \
             VALUES ('ZQG', 'ZG', NOW(), \
             '{\"total\":40,\"delayed\":4,\"cancelled\":1,\"skipped\":0,\"avg_delay_minutes\":2.5}')",
        )
        .execute(&pool)
        .await
        .expect("seed full-coverage-only fixture row");

        // Same as the previous test, but full_coverage_enabled is false --
        // proves the gate, not just row presence, controls the wire output.
        let router: axum::Router =
            crate::app::Router::new()
                .merge(router())
                .with_state(test_app_with_lines(
                    pool.clone(),
                    vec![gating_line("ZQG", "ZG", false)],
                ));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQG/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["operator"], "ZG");
        assert_eq!(
            json[0]["fullCoverageAvailability"],
            serde_json::json!({"state": "not-enabled"})
        );
        assert!(json[0].get("fullCoverageStats").is_none());

        delete_fixture(&pool, "ZQG").await;
        delete_full_coverage_fixture(&pool, "ZQG", "ZG").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_full_coverage_enabled_default_true_enables_a_gate_disabled_line()
    {
        let pool = connect().await;
        delete_fixture(&pool, "ZQJ").await;
        delete_full_coverage_fixture(&pool, "ZQJ", "ZJ").await;

        sqlx::query(
            "INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats) \
             VALUES ('ZQJ', 'ZJ', NOW(), \
             '{\"total\":40,\"delayed\":4,\"cancelled\":1,\"skipped\":0,\"avg_delay_minutes\":2.5}')",
        )
        .execute(&pool)
        .await
        .expect("seed full-coverage-only fixture row");

        // Same fixture as the gate-disabled test above (the line's own
        // full_coverage_enabled stays false), but this app is built with
        // full_coverage_enabled_default: true -- proves the global
        // override alone is enough to flip the wire output on, the new
        // case this task adds.
        let router: axum::Router = crate::app::Router::new().merge(router()).with_state(
            test_app_with_lines_and_full_coverage_default(
                pool.clone(),
                vec![gating_line("ZQJ", "ZJ", false)],
                true,
            ),
        );
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQJ/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["operator"], "ZJ");
        assert_eq!(
            json[0]["fullCoverageAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert_eq!(
            json[0]["fullCoverageStats"],
            serde_json::json!({
                "total": 40, "delayed": 4, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 2.5
            })
        );

        delete_fixture(&pool, "ZQJ").await;
        delete_full_coverage_fixture(&pool, "ZQJ", "ZJ").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_sample_and_full_coverage_present_simultaneously() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQH").await;
        delete_full_coverage_fixture(&pool, "ZQH", "ZH").await;

        let departures = serde_json::json!([
            {
                "service_id": "svc-1", "operator": "ZH", "destination_crs": "WAT",
                "scheduled": "10:00", "estimated": "10:07", "is_cancelled": false,
                "delay_minutes": 7, "skipped_stations": []
            },
            {
                "service_id": "svc-2", "operator": "ZH", "destination_crs": "WAT",
                "scheduled": "10:10", "estimated": "On time", "is_cancelled": false,
                "delay_minutes": 0, "skipped_stations": []
            },
            {
                "service_id": "svc-3", "operator": "ZH", "destination_crs": "WAT",
                "scheduled": "10:20", "estimated": "On time", "is_cancelled": false,
                "delay_minutes": 0, "skipped_stations": []
            }
        ]);
        sqlx::query(
            "INSERT INTO station_samples (crs, polled_at, departures) VALUES ('ZQH', NOW(), $1)",
        )
        .bind(departures)
        .execute(&pool)
        .await
        .expect("seed station_samples fixture row");

        sqlx::query(
            "INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats) \
             VALUES ('ZQH', 'ZH', NOW(), \
             '{\"total\":50,\"delayed\":6,\"cancelled\":1,\"skipped\":0,\"avg_delay_minutes\":2.1}')",
        )
        .execute(&pool)
        .await
        .expect("seed station_full_coverage_samples fixture row");

        let router: axum::Router =
            crate::app::Router::new()
                .merge(router())
                .with_state(test_app_with_lines(
                    pool.clone(),
                    vec![gating_line("ZQH", "ZH", true)],
                ));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQH/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["operator"], "ZH");
        assert_eq!(
            json[0]["sampleAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert_eq!(
            json[0]["sampleStats"],
            serde_json::json!({
                "total": 3, "delayed": 1, "cancelled": 0, "skipped": 0, "avgDelayMinutes": 7.0 / 3.0
            })
        );
        assert_eq!(
            json[0]["fullCoverageAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert_eq!(
            json[0]["fullCoverageStats"],
            serde_json::json!({
                "total": 50, "delayed": 6, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 2.1
            })
        );

        delete_fixture(&pool, "ZQH").await;
        delete_full_coverage_fixture(&pool, "ZQH", "ZH").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_sample_stats -- --ignored --test-threads=1`"]
    async fn station_sample_stats_neither_table_has_a_row_is_still_404() {
        // Regression check on the widened 404 gate: a fresh fixture CRS
        // with no station_samples row, no station_full_coverage_samples
        // row, and no line (gating or otherwise) covering it must still
        // 404, exactly as before this plan's change.
        let pool = connect().await;
        delete_fixture(&pool, "ZQI").await;
        delete_full_coverage_fixture(&pool, "ZQI", "ZI").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQI/sample-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
