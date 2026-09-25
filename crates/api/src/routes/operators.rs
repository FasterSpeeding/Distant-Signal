//! `GET /public/operators` (+ `/operators/{code}` detail): per-operator
//! status rollups backing the Phase 3 operators list page and operator
//! pinning. Unauthenticated -- same `public_router()` posture as
//! `lines.rs`/`reference.rs` (every real ATOC code, and the synthetic
//! `"TfL"` row, are public reference concepts; nothing here varies by
//! caller identity, unlike `preferences.rs`). See
//! docs/superpowers/specs/2026-09-22-operator-overview-design.md §C and
//! docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::operators;
use crate::render::sample_stats_json;

pub fn router() -> Router {
    Router::new()
        .route("/operators", axum::routing::get(list_operators))
        .route("/operators/{code}", axum::routing::get(get_operator))
}

async fn list_operators(State(app): State<App>) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rollups = operators::all_operator_rollups(&app.database, &app.config.lines)
        .await
        .map_err(internal_error)?;
    Ok(Json(rollups.iter().map(operator_rollup_json).collect()))
}

async fn get_operator(
    State(app): State<App>,
    Path(code): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let rollup = operators::operator_rollup(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?;
    let Some(rollup) = rollup else {
        return Err((StatusCode::NOT_FOUND, "operator not found".to_string()));
    };
    Ok(Json(operator_rollup_json(&rollup)))
}

/// Hand-built camelCase JSON -- NOT a derived `#[serde(rename_all = "camelCase")]`
/// struct embedding `common::SampleStats` directly, which would leak
/// `avg_delay_minutes` (snake_case) one level down (a parent struct's
/// `rename_all` does not recurse into a nested type with no rename
/// attribute of its own). Same rationale, and the same reused
/// `sample_stats_json` helper, as `routes/station_stats.rs`'s own
/// `get_station_sample_stats` -- see this plan's Judgment Call 1.
fn operator_rollup_json(r: &operators::OperatorRollup) -> Value {
    let mut out = json!({
        "code": r.code,
        "name": r.name,
        "lineIds": r.line_ids,
        "worstSeverity": r.worst_severity as i32,
        "reason": r.reason,
        "worstLineId": r.worst_line_id,
        "worstLineName": r.worst_line_name,
        "computedAt": r.computed_at,
    });
    if let Some(stats) = &r.sample_stats {
        out["sampleStats"] = sample_stats_json(stats);
    }
    out
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "operators rollup query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Copied verbatim from `crates/api/src/routes/station_stats.rs`'s own
    /// `db_tests::test_app_with_lines_and_full_coverage_default` (that
    /// module's own doc comment: colocated per-file rather than shared,
    /// until a third file needs it too -- this is the second, not the
    /// third). Every field an inert placeholder except `database`/`lines`,
    /// which the caller supplies.
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
            metrics_port: 9091,
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

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Reserved `Z…` fixture-code namespace, matching this file's own
    /// `station_stats.rs` sibling test convention -- picks a `tocs` code
    /// unlikely to collide with a real ATOC code.
    fn gating_line(id: &str, operator: &str) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: vec![operator.to_string()],
            stations: vec![common::Station {
                crs: "ZZZ".to_string(),
                tiploc: None,
                role: "minor".to_string(),
                segment: None,
            }],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

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

    async fn seed_line_status(
        pool: &PgPool,
        line_id: &str,
        operators: &[&str],
        statuses: serde_json::Value,
    ) {
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ($1, $1, 'national-rail', $2, $3, 'aggregator') \
             ON CONFLICT (line_id) DO UPDATE SET operators = EXCLUDED.operators, statuses = EXCLUDED.statuses",
        )
        .bind(line_id)
        .bind(operators)
        .bind(statuses)
        .execute(pool)
        .await
        .expect("seed fixture line_status row");
    }

    async fn cleanup(pool: &PgPool, toc_code: &str, line_id: &str) {
        sqlx::query("DELETE FROM tocs WHERE atoc_code = $1")
            .bind(toc_code)
            .execute(pool)
            .await
            .expect("cleanup fixture toc");
        sqlx::query("DELETE FROM line_status WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture line_status row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                list_operators_returns_a_rollup_for_a_toc_with_a_matching_line \
                -- --ignored --test-threads=1`"]
    async fn list_operators_returns_a_rollup_for_a_toc_with_a_matching_line() {
        let pool = connect().await;
        seed_toc(&pool, "ZA", "Z Test Rail").await;
        seed_line_status(
            &pool,
            "ztest-operators-line",
            &["ZA"],
            serde_json::json!([{
                "severity": 9, "reason": "minor delays", "validity": {"from_date": "2026-01-01T00:00:00Z", "to_date": null, "is_now": true},
                "data_quality": "knowledgebase", "sample_availability": {"state": "no-coverage"},
                "full_coverage_availability": {"state": "not-enabled"}
            }]),
        )
        .await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(
                pool.clone(),
                vec![gating_line("ztest-operators-line", "ZA")],
            ));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators")
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
        let za = json
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["code"] == "ZA")
            .expect("ZA rollup present");
        assert_eq!(za["name"], "Z Test Rail");
        assert_eq!(za["lineIds"], serde_json::json!(["ztest-operators-line"]));
        assert_eq!(za["worstSeverity"], 9);
        assert_eq!(za["worstLineId"], "ztest-operators-line");
        assert_eq!(za["worstLineName"], "ztest-operators-line");

        cleanup(&pool, "ZA", "ztest-operators-line").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_operator_unknown_code_is_404 -- --ignored --test-threads=1`"]
    async fn get_operator_unknown_code_is_404() {
        let pool = connect().await;
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), vec![]));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators/ZZ-NOT-REAL")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_operator_a_toc_with_zero_matching_lines_is_404 -- --ignored --test-threads=1`"]
    async fn get_operator_a_toc_with_zero_matching_lines_is_404() {
        // Judgment Call 4: a real tocs row with no matching line is
        // omitted, same as an unknown code.
        let pool = connect().await;
        seed_toc(&pool, "ZB", "Z Ghost Rail").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), vec![]));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators/ZB")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        sqlx::query("DELETE FROM tocs WHERE atoc_code = 'ZB'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
