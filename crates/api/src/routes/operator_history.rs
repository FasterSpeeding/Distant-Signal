//! `GET /public/operators/{code}/stats/...` and `GET /public/network/stats/...`
//! -- the operator- and network-scoped Trends rollup, per
//! docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md.
//! Four granularities per scope (daily, half-hourly, hourly, six-hourly),
//! mirroring the four per-line routes in `line_status.rs` exactly --
//! same response shape (reusing that file's own JSON mappers, now
//! `pub(crate)`), same `{from}/to/{to}` path-segment idiom. Deliberately
//! unauthenticated (no `OptionalAuthenticatedUser`, no
//! `empty_if_unreadable`-style gate): every line id these routes ever
//! touch comes from `operators::operator_rollup`/`network_line_ids`, both
//! of which exclude custom lines unconditionally -- see this plan's
//! Judgment Call 7. `operator_rollup`'s `line_ids` is itself built by
//! construction (see `crates/api/src/data/operators.rs`'s own module doc)
//! from only `app.config.lines`' static catalogue ids and TfL-ingested
//! summary ids, so a private custom line's id is never in the set these
//! routes ever query with. Nested under `/public` (`routes::public_router()`),
//! unlike `line_status.rs`'s TfL-shape-compatible routes, since there is no
//! TfL API compatibility concern for a wholly new surface.
//!
//! No Timeline-equivalent, no coverage-stats sibling -- both are
//! deliberate Non-goals of the plan above (spec §D.3; coverage tables
//! have no real producer yet at any scope).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router as AxumRouter};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::{operators, queries};
use crate::routes::line_status::{
    daily_stats_to_json, half_hourly_stats_to_json, sub_daily_stats_to_json,
};

pub fn router() -> Router {
    AxumRouter::new()
        .route(
            "/operators/{code}/stats/{from}/to/{to}",
            axum::routing::get(get_operator_daily_stats),
        )
        .route(
            "/operators/{code}/stats/half-hourly/{from}/to/{to}",
            axum::routing::get(get_operator_half_hourly_stats),
        )
        .route(
            "/operators/{code}/stats/hourly/{from}/to/{to}",
            axum::routing::get(get_operator_hourly_stats),
        )
        .route(
            "/operators/{code}/stats/six-hourly/{from}/to/{to}",
            axum::routing::get(get_operator_six_hourly_stats),
        )
        .route(
            "/network/stats/{from}/to/{to}",
            axum::routing::get(get_network_daily_stats),
        )
        .route(
            "/network/stats/half-hourly/{from}/to/{to}",
            axum::routing::get(get_network_half_hourly_stats),
        )
        .route(
            "/network/stats/hourly/{from}/to/{to}",
            axum::routing::get(get_network_hourly_stats),
        )
        .route(
            "/network/stats/six-hourly/{from}/to/{to}",
            axum::routing::get(get_network_six_hourly_stats),
        )
}

/// Every catalogue (National Rail) line id -- the "network" scope's own
/// line-id set. Deliberately NOT `queries::tfl_line_summaries` -- TfL
/// lines never accrue rows in `line_status_daily_stats`/
/// `line_status_half_hourly_stats` at all (see this plan's Judgment Call
/// 5: the aggregator's own `record_daily_stats`/`record_half_hourly_stats`
/// pass only ever iterates catalogue + custom lines, never TfL), so
/// including `tfl-`-prefixed ids here would add ids to the `= ANY($1)`
/// list that can never match a row -- harmless, but pointless. Pure and
/// synchronous so it's unit-testable without a database.
fn network_line_ids(catalogue_lines: &[common::LineDefinition]) -> Vec<String> {
    catalogue_lines.iter().map(|line| line.id.clone()).collect()
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "operator/network history query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

async fn get_operator_daily_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::operator_rollup(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?
        .map(|rollup| rollup.line_ids)
        .unwrap_or_default();
    let rows = queries::daily_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(daily_stats_to_json).collect()))
}

async fn get_operator_half_hourly_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::operator_rollup(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?
        .map(|rollup| rollup.line_ids)
        .unwrap_or_default();
    let rows = queries::half_hourly_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(half_hourly_stats_to_json).collect(),
    ))
}

async fn get_operator_hourly_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::operator_rollup(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?
        .map(|rollup| rollup.line_ids)
        .unwrap_or_default();
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 60)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(sub_daily_stats_to_json).collect(),
    ))
}

async fn get_operator_six_hourly_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::operator_rollup(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?
        .map(|rollup| rollup.line_ids)
        .unwrap_or_default();
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 360)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(sub_daily_stats_to_json).collect(),
    ))
}

async fn get_network_daily_stats(
    State(app): State<App>,
    Path((from, to)): Path<(chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::daily_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(daily_stats_to_json).collect()))
}

async fn get_network_half_hourly_stats(
    State(app): State<App>,
    Path((from, to)): Path<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::half_hourly_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(half_hourly_stats_to_json).collect(),
    ))
}

async fn get_network_hourly_stats(
    State(app): State<App>,
    Path((from, to)): Path<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 60)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(sub_daily_stats_to_json).collect(),
    ))
}

async fn get_network_six_hourly_stats(
    State(app): State<App>,
    Path((from, to)): Path<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 360)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(sub_daily_stats_to_json).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_line_ids_lists_every_catalogue_line_and_nothing_else() {
        let lines = vec![
            common::LineDefinition {
                id: "a".to_string(),
                name: "A".to_string(),
                mode: "national-rail".to_string(),
                category: "main-line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec![],
                sample_stations: vec![],
                match_keywords: vec![],
                excluded_keywords: vec![],
                severity_overrides: Default::default(),
                destination_crs_filter: vec![],
                headcode_prefixes: vec![],
                full_coverage_enabled: false,
            },
            common::LineDefinition {
                id: "b".to_string(),
                name: "B".to_string(),
                mode: "national-rail".to_string(),
                category: "main-line".to_string(),
                operators: vec!["GW".to_string()],
                stations: vec![],
                sample_stations: vec![],
                match_keywords: vec![],
                excluded_keywords: vec![],
                severity_overrides: Default::default(),
                destination_crs_filter: vec![],
                headcode_prefixes: vec![],
                full_coverage_enabled: false,
            },
        ];
        assert_eq!(
            network_line_ids(&lines),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    // Fields copied verbatim from operators.rs's own db_tests::test_app
    // (read directly while implementing this task,
    // crates/api/src/routes/operators.rs:95-156) -- every placeholder
    // inert except `lines`, which each test supplies.
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
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
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
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
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
        };

        std::sync::Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
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

    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
    }

    fn catalogue_line(id: &str, operators: &[&str]) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: format!("Test {id}"),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            stations: vec![],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Ported from `operators.rs`'s own `db_tests::seed_toc` (read directly
    /// while fixing this test after code review: `operators::operator_rollup`
    /// -- which `get_operator_daily_stats` and its granularity siblings all
    /// call -- resolves a code to line ids ONLY via a real `tocs` row
    /// (`reference::get_all_tocs`), never from a `LineDefinition.operators`
    /// entry alone. A test that seeds only `line_status_daily_stats` and a
    /// catalogue `LineDefinition` (as this test originally did) still
    /// resolves to an empty line-id set against a live database, because
    /// there is no matching `tocs` row for the operator code requested.
    async fn seed_toc(pool: &PgPool, code: &str, name: &str) {
        sqlx::query(
            "INSERT INTO tocs (atoc_code, name, legal_name, fetched_at) VALUES ($1, $2, $2, NOW()) \
             ON CONFLICT (atoc_code) DO UPDATE SET name = EXCLUDED.name",
        )
        .bind(code)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed fixture toc");
    }

    /// Ported from `operators.rs`'s own `db_tests::seed_line_status`. Needed
    /// alongside `seed_toc`: `operator_rollup`'s line-id set is the
    /// intersection of "has a matching `tocs` row" AND "has a `line_status`
    /// row whose `operators` column carries that code" (see
    /// `crates/api/src/data/operators.rs`'s `public_line_status_rows` /
    /// `all_operator_rollups`) -- a catalogue `LineDefinition.operators`
    /// entry alone, with no corresponding `line_status` row, still resolves
    /// to zero matching lines.
    async fn seed_line_status(pool: &PgPool, line_id: &str, operators: &[&str]) {
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ($1, $1, 'national-rail', $2, '[]'::jsonb, 'aggregator') \
             ON CONFLICT (line_id) DO UPDATE SET operators = EXCLUDED.operators, statuses = EXCLUDED.statuses",
        )
        .bind(line_id)
        .bind(operators)
        .execute(pool)
        .await
        .expect("seed fixture line_status row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_operator_daily_stats -- --ignored --test-threads=1`"]
    async fn get_operator_daily_stats_sums_only_that_operators_lines() {
        let pool = connect().await;
        // Reserved `Z…` fixture-code namespace, matching `operators.rs`'s
        // own test convention -- deliberately NOT the real "SW" (South
        // Western Railway) ATOC code, since this test's `seed_toc`/cleanup
        // pair would upsert-then-delete a real `tocs` row of that code if
        // one already existed in whatever database this runs against.
        seed_toc(&pool, "ZR", "Z Test Route Railway").await;
        seed_line_status(&pool, "TEST-ROUTE-OP-A", &["ZR"]).await;
        seed_line_status(&pool, "TEST-ROUTE-OP-B", &["ZR"]).await;
        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-ROUTE-OP-A', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-ROUTE-OP-B', '2026-08-01', 8, 80, 3, 0, 1, 79, 60.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let lines = vec![
            catalogue_line("TEST-ROUTE-OP-A", &["ZR"]),
            catalogue_line("TEST-ROUTE-OP-B", &["ZR"]),
        ];
        let router = test_router(test_app(pool.clone(), lines));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators/ZR/stats/2026-08-01/to/2026-08-01")
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
        let rows = json.as_array().expect("array response");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["total"], 180);
        assert_eq!(rows[0]["delayed"], 8);

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id LIKE 'TEST-ROUTE-OP-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");
        sqlx::query("DELETE FROM line_status WHERE line_id LIKE 'TEST-ROUTE-OP-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture line_status rows");
        sqlx::query("DELETE FROM tocs WHERE atoc_code = 'ZR'")
            .execute(&pool)
            .await
            .expect("cleanup fixture toc");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_operator_daily_stats_an_unknown_operator -- --ignored --test-threads=1`"]
    async fn get_operator_daily_stats_an_unknown_operator_code_is_200_empty_not_404() {
        let pool = connect().await;
        let router = test_router(test_app(
            pool,
            vec![catalogue_line("TEST-ROUTE-OP-C", &["SW"])],
        ));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators/NOSUCHCODE/stats/2026-08-01/to/2026-08-01")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "an unknown operator code is an empty result, not a 404 -- matching \
             daily_stats_for_range's own unknown-line_id convention"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_network_daily_stats -- --ignored --test-threads=1`"]
    async fn get_network_daily_stats_sums_every_catalogue_line() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-ROUTE-NET-A', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-ROUTE-NET-B', '2026-08-01', 8, 80, 3, 0, 1, 79, 60.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let lines = vec![
            catalogue_line("TEST-ROUTE-NET-A", &["SW"]),
            catalogue_line("TEST-ROUTE-NET-B", &["GW"]),
        ];
        let router = test_router(test_app(pool.clone(), lines));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/network/stats/2026-08-01/to/2026-08-01")
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
        let rows = json.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]["total"], 180,
            "both catalogue lines summed, regardless of operator"
        );

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id LIKE 'TEST-ROUTE-NET-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");
    }
}
