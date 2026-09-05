//! `GET /public/stanox-crs`: a bulk, unauthenticated public mirror of the
//! live STANOX->CRS->TIPLOC->station-name reference table. Entirely
//! independent of `/public/stations`/`search_stations`
//! (`crates/api/src/routes/reference.rs`, `crates/api/src/data/reference.rs`)
//! -- different table (`stanox_crs`, not `stations`), different query,
//! different file; this route adds no coupling between the two. See
//! docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md
//! Decision 4.
//!
//! Reads the exact same `queries::list_stanox_crs` the existing
//! internal-oauth-gated `GET /private/stanox-crs`
//! (`crates/api/src/routes/ingest.rs`) already uses -- same query, same
//! "the full current table, not a freshness timestamp" shape, different
//! (public, no-credential) mount point. No pagination, matching
//! `routes::island_of_ireland`'s own "the whole catalogue is a few hundred
//! rows at most" precedent for an unauthenticated whole-table GET.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/stanox-crs", axum::routing::get(list_stanox_crs))
}

async fn list_stanox_crs(
    State(app): State<App>,
) -> Result<Json<Vec<common::StanoxCrsRecord>>, (StatusCode, String)> {
    let rows = queries::list_stanox_crs(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "public stanox-crs mirror query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    /// Guards against a future `#[serde(rename_all = "camelCase")]`
    /// silently being added to `common::StanoxCrsRecord` (which would
    /// change this route's response shape without anyone touching this
    /// file) -- this route deliberately mirrors `GET /private/stanox-crs`'s
    /// existing snake_case shape unchanged, per this plan's Global
    /// Constraints.
    #[test]
    fn stanox_crs_record_serializes_as_snake_case_unchanged() {
        let record = common::StanoxCrsRecord {
            stanox: "12345".to_string(),
            crs: "WAT".to_string(),
            tiploc: "WATRLMN".to_string(),
            station_name: "London Waterloo".to_string(),
            source_sequence: 7,
        };
        let value = serde_json::to_value(&record).expect("serialize StanoxCrsRecord");
        assert_eq!(
            value,
            serde_json::json!({
                "stanox": "12345",
                "crs": "WAT",
                "tiploc": "WATRLMN",
                "station_name": "London Waterloo",
                "source_sequence": 7
            })
        );
    }
}

/// HTTP-layer tests exercised against a live database -- mirrors
/// `routes::lines::db_tests`'s seed/assert/delete pattern and its
/// `test_app` helper (copied here rather than shared, matching this
/// crate's own established convention of colocating this helper per-file
/// until a shared module is actually warranted -- see
/// `routes::lines::db_tests::test_app`'s own doc comment for the same
/// reasoning stated the first time this was copied).
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

    fn test_app(pool: PgPool) -> App {
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
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
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

    async fn delete_fixture(pool: &PgPool, stanox: &str) {
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = $1")
            .bind(stanox)
            .execute(pool)
            .await
            .expect("cleanup fixture stanox_crs row");
    }

    async fn seed_fixture(pool: &PgPool, stanox: &str, crs: &str, tiploc: &str, name: &str) {
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ($1, $2, $3, $4, 1) \
             ON CONFLICT (stanox) DO UPDATE SET \
                crs = EXCLUDED.crs, tiploc = EXCLUDED.tiploc, station_name = EXCLUDED.station_name",
        )
        .bind(stanox)
        .bind(crs)
        .bind(tiploc)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed fixture stanox_crs row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                stanox_crs_route_is_unauthenticated_and_returns_seeded_rows \
                -- --ignored --test-threads=1`"]
    async fn stanox_crs_route_is_unauthenticated_and_returns_seeded_rows() {
        let pool = connect().await;
        delete_fixture(&pool, "9ZTEST01").await;
        delete_fixture(&pool, "9ZTEST02").await;

        seed_fixture(&pool, "9ZTEST01", "ZTA", "ZTESTTPA", "Test Alpha").await;
        seed_fixture(&pool, "9ZTEST02", "ZTB", "ZTESTTPB", "Test Beta").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        // No Authorization/Cookie header at all -- proves this route needs
        // no credential, per this plan's Global Constraints.
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stanox-crs")
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
        let rows = json.as_array().expect("response is a JSON array");

        let find = |stanox: &str| {
            rows.iter()
                .find(|r| r.get("stanox").and_then(Value::as_str) == Some(stanox))
        };
        let alpha = find("9ZTEST01").expect("fixture 9ZTEST01 present in response");
        assert_eq!(alpha["crs"], "ZTA");
        assert_eq!(alpha["tiploc"], "ZTESTTPA");
        assert_eq!(alpha["station_name"], "Test Alpha");
        let beta = find("9ZTEST02").expect("fixture 9ZTEST02 present in response");
        assert_eq!(beta["crs"], "ZTB");

        delete_fixture(&pool, "9ZTEST01").await;
        delete_fixture(&pool, "9ZTEST02").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                stanox_crs_route_reflects_an_update_to_an_existing_row \
                -- --ignored --test-threads=1`"]
    async fn stanox_crs_route_reflects_an_update_to_an_existing_row() {
        // Proves this is a live read of the current table, not a cached or
        // point-in-time snapshot -- matching `queries::list_stanox_crs`'s
        // own doc comment ("the full current table").
        let pool = connect().await;
        delete_fixture(&pool, "9ZTEST03").await;
        seed_fixture(&pool, "9ZTEST03", "ZTC", "ZTESTTPC", "Test Gamma Old Name").await;
        seed_fixture(&pool, "9ZTEST03", "ZTC", "ZTESTTPC", "Test Gamma New Name").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stanox-crs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let rows = json.as_array().expect("response is a JSON array");
        let gamma = rows
            .iter()
            .find(|r| r.get("stanox").and_then(Value::as_str) == Some("9ZTEST03"))
            .expect("fixture 9ZTEST03 present in response");
        assert_eq!(gamma["station_name"], "Test Gamma New Name");

        delete_fixture(&pool, "9ZTEST03").await;
    }
}
