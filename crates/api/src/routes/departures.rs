//! `GET /public/stations/{crs}/departures`: today's live departure board
//! for `crs`, straight from `station_samples`, no aggregation. Backs the
//! trip-search picker on `/track`
//! (docs/superpowers/specs/2026-09-03-trip-search-design.md). Sibling to
//! `station_stats.rs`, same `latest_station_sample` read, same honesty
//! split, deliberately not merged into that file: this returns raw rows
//! for a picker, not computed per-operator stats -- different callers,
//! different wire shapes, no shared logic beyond the DB read itself.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::queries;
use crate::render::station_departure_json;

pub fn router() -> Router {
    Router::new().route(
        "/stations/{crs}/departures",
        axum::routing::get(get_station_departures),
    )
}

/// 404 when `station_samples` has no row for `crs` at all -- identical
/// honesty split to `station_stats.rs::get_station_sample_stats`. `200 []`
/// is the same "row exists, board is genuinely empty right now" fact that
/// route already draws.
async fn get_station_departures(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let Some(sample) = queries::latest_station_sample(&app.database, &crs)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no sample data collected for station: {crs}"),
        ));
    };

    // Order preserved exactly as stored -- `parse_departures` never
    // re-sorts (poller-ldbws/src/schema.rs), and RDM's own board is
    // already chronological by convention. No new sort introduced here.
    Ok(Json(
        sample
            .departures
            .iter()
            .map(station_departure_json)
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "station departures query failed");
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
        // `station_stats.rs`'s own probe-router precedent for the same
        // concern.
        async fn probe(Path(crs): Path<String>) -> String {
            crs
        }

        let app: axum::Router =
            axum::Router::new().route("/stations/{crs}/departures", axum::routing::get(probe));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/stations/EDB/departures")
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
/// `station_stats.rs::db_tests`'s seed/assert/delete pattern and its
/// `test_app` helper for constructing a real `App` around a live pool
/// without needing every other part of `AppState::init` (OIDC discovery,
/// etc). Uses `ZQT`/`ZQU` fixture CRS codes -- deliberately not `ZQQ`/
/// `ZQR`/`ZQS`, which `station_stats.rs`'s own db_tests already claims in
/// the same reserved `Z…` fixture namespace, since both files' tests may
/// run against the same test database.
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

    /// Copied from `routes::station_stats::db_tests::test_app` (that
    /// module's own doc comment: colocated per-file rather than shared,
    /// until a third file needs it too). Every field an inert placeholder
    /// except `database`, which the caller supplies -- this route touches
    /// nothing else on `App`.
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
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
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

    async fn delete_fixture(pool: &PgPool, crs: &str) {
        sqlx::query("DELETE FROM station_samples WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .expect("cleanup fixture station_samples row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                departures -- --ignored --test-threads=1`"]
    async fn departures_no_row_for_crs_is_404_naming_the_crs() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQT").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQT/departures")
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
        assert!(body.contains("ZQT"), "404 body should name the CRS: {body}");

        delete_fixture(&pool, "ZQT").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                departures -- --ignored --test-threads=1`"]
    async fn departures_a_row_present_with_empty_departures_is_200_empty_array() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQT").await;

        sqlx::query(
            "INSERT INTO station_samples (crs, polled_at, departures) VALUES ('ZQT', NOW(), '[]')",
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
                    .uri("/stations/ZQT/departures")
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

        delete_fixture(&pool, "ZQT").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                departures -- --ignored --test-threads=1`"]
    async fn departures_two_rows_render_camel_case_unfiltered_in_storage_order() {
        let pool = connect().await;
        delete_fixture(&pool, "ZQU").await;

        // Insertion order deliberately NOT chronological by `scheduled` --
        // proves the route preserves storage order rather than introducing
        // a new sort (design doc Decision 2). One cancelled entry with a
        // `cancelReason` set, one on-time entry with `cancelReason: null`.
        let departures = serde_json::json!([
            {
                "service_id": "svc-2", "operator": "ZB", "destination_crs": "BSK",
                "scheduled": "14:40", "estimated": "14:47", "is_cancelled": false,
                "delay_minutes": 7, "skipped_stations": []
            },
            {
                "service_id": "svc-1", "operator": "ZA", "destination_crs": "WAT",
                "scheduled": "14:20", "estimated": "Cancelled", "is_cancelled": true,
                "delay_minutes": 0, "cancel_reason": "fleet issue", "skipped_stations": []
            }
        ]);
        sqlx::query(
            "INSERT INTO station_samples (crs, polled_at, departures) VALUES ('ZQU', NOW(), $1)",
        )
        .bind(departures)
        .execute(&pool)
        .await
        .expect("seed two-departure fixture row");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stations/ZQU/departures")
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
        // Storage order preserved: svc-2 (delayed, ZB) inserted first,
        // svc-1 (cancelled, ZA) second -- not re-sorted by `scheduled`.
        assert_eq!(json[0]["serviceId"], "svc-2");
        assert_eq!(json[0]["operator"], "ZB");
        assert_eq!(json[0]["destinationCrs"], "BSK");
        assert_eq!(json[0]["scheduled"], "14:40");
        assert_eq!(json[0]["isCancelled"], false);
        assert_eq!(json[0]["delayMinutes"], 7);
        assert!(json[0]["cancelReason"].is_null());

        assert_eq!(json[1]["serviceId"], "svc-1");
        assert_eq!(json[1]["operator"], "ZA");
        assert_eq!(json[1]["isCancelled"], true);
        assert_eq!(json[1]["cancelReason"], "fleet issue");

        delete_fixture(&pool, "ZQU").await;
    }
}
