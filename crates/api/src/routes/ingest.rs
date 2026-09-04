//! Ingestion endpoints for `private_router()`. Each poller POSTs a `Vec<T>`
//! snapshot once per poll cycle; these handlers just deserialize the body
//! (via axum's `Json<T>` extractor — no hand-rolled body validation) and
//! hand it to the matching upsert query.
//!
//! `/tfl-line-status` is the odd one out: its batch is already-computed
//! line status from TfL rather than raw upstream data, so its upsert
//! targets `line_status`/`line_status_history` directly (see
//! `queries::upsert_tfl_line_status`).
//!
//! Each POST route also has a same-path GET counterpart (see `router()`)
//! returning when that table was last successfully populated. Pollers call
//! it once at startup, before their poll loop begins, to skip an
//! immediately-redundant first fetch if the existing data is still fresh —
//! see `common::ingest::time_until_next_poll`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use common::ingest::LastFetchedResponse;
use common::{
    IncidentMessage, LineStatusReport, StationFullCoverageSample, StationReference, StationSample,
    TocReference,
};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::ScheduleNetworkDeparturesRow;
use crate::data::train_tracking as queries_train_tracking;

pub fn router() -> Router {
    Router::new()
        .route(
            "/incidents",
            axum::routing::get(get_incidents_last_fetched).post(post_incidents),
        )
        .route(
            "/stations",
            axum::routing::get(get_stations_last_fetched).post(post_stations),
        )
        .route(
            "/tocs",
            axum::routing::get(get_tocs_last_fetched).post(post_tocs),
        )
        .route(
            "/station-samples",
            axum::routing::get(get_station_samples_last_fetched).post(post_station_samples),
        )
        .route(
            "/station-full-coverage-samples",
            axum::routing::get(get_station_full_coverage_samples_last_fetched)
                .post(post_station_full_coverage_samples),
        )
        .route(
            "/tfl-line-status",
            axum::routing::get(get_tfl_line_status_last_fetched).post(post_tfl_line_status),
        )
        .route("/train-events", axum::routing::post(post_train_events))
        .route(
            "/tracked-trains",
            axum::routing::get(get_active_tracked_trains),
        )
        .route(
            "/schedule-feed-ingests",
            axum::routing::get(get_schedule_feed_last_fetched).post(post_schedule_feed_ingest),
        )
        .route(
            "/stanox-crs",
            axum::routing::get(get_stanox_crs).post(post_stanox_crs),
        )
        .route(
            "/schedule-line-population",
            axum::routing::get(get_schedule_line_population).post(post_schedule_line_population),
        )
        .route(
            "/full-coverage-stats",
            axum::routing::get(get_full_coverage_stats_last_fetched).post(post_full_coverage_stats),
        )
        .route(
            "/schedule-network-departures",
            axum::routing::post(post_schedule_network_departures),
        )
}

#[derive(Debug, Serialize)]
struct UpsertResponse {
    upserted: u64,
}

async fn get_incidents_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_incidents_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_stations_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_stations_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_tocs_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_tocs_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_station_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_station_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn get_tfl_line_status_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_tfl_line_status_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_incidents(
    State(app): State<App>,
    Json(incidents): Json<Vec<IncidentMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_incidents(&app.database, &app.redis, &incidents)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn post_stations(
    State(app): State<App>,
    Json(stations): Json<Vec<StationReference>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_stations(&app.database, &stations)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn post_station_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<StationSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_station_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn get_station_full_coverage_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_station_full_coverage_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_station_full_coverage_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<StationFullCoverageSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_station_full_coverage_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn post_tocs(
    State(app): State<App>,
    Json(tocs): Json<Vec<TocReference>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tocs(&app.database, &tocs)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// Unlike the other four ingest routes, this one writes the aggregator's
/// output table directly. That is not a shortcut: TfL publishes finished
/// line status, so there is nothing for the aggregator to infer from
/// incidents or departure boards, and routing it through that crate would
/// mean inventing a second input table for data that is already in its
/// final shape. The two writers stay out of each other's way via
/// `line_status.source` and the `tfl-` line-id prefix.
async fn post_tfl_line_status(
    State(app): State<App>,
    Json(reports): Json<Vec<LineStatusReport>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tfl_line_status(&app.database, &reports)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `trust-consumer`'s per-poll-cycle batch of TRUST-derived events for
/// tracked trains -- see `queries_train_tracking::upsert_train_event`.
async fn post_train_events(
    State(app): State<App>,
    Json(events): Json<Vec<common::TrainMovementEventMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    for event in &events {
        queries_train_tracking::upsert_train_event(&app.database, event)
            .await
            .map_err(internal_error)?;
    }
    Ok(Json(UpsertResponse {
        upserted: events.len() as u64,
    }))
}

/// `trust-consumer`'s periodic reference reload -- pending and
/// resolved-but-not-completed tracked trains, so it can recognize incoming
/// TRUST messages against them after a restart. See
/// `queries_train_tracking::list_active_tracked_trains`.
async fn get_active_tracked_trains(
    State(app): State<App>,
) -> Result<Json<Vec<common::TrackedTrainRef>>, (StatusCode, String)> {
    let rows = queries_train_tracking::list_active_tracked_trains(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

/// `schedule-ingest`'s per-delivery record of one successfully-verified CIF
/// SCHEDULE feed delivery. Unlike the other ingest routes this isn't a
/// per-poll-cycle batch of reference data -- it's one row per delivery,
/// recorded once a stable `.zip` delivery has been extracted (see
/// `crates/schedule-ingest`).
///
/// `delivered_at` is the delivery zip's own mtime -- the real identity of
/// "which delivery is this" now that there is no sequence number (see
/// `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`).
/// `ingested_at` is when this process actually happened to be processed,
/// kept only as separate observability data.
#[derive(Debug, Deserialize)]
struct ScheduleFeedIngestRequest {
    delivered_at: chrono::DateTime<chrono::Utc>,
    ingested_at: chrono::DateTime<chrono::Utc>,
    files: Vec<ScheduleFeedFile>,
}

/// One file observed as part of a schedule-feed delivery. `bytes` is the
/// size `schedule-ingest` itself observed on disk once stable, not a
/// manifest-declared size -- the real manifest format has no such field.
#[derive(Debug, Deserialize, Serialize)]
struct ScheduleFeedFile {
    name: String,
    bytes: u64,
}

async fn get_schedule_feed_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_schedule_feed_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_schedule_feed_ingest(
    State(app): State<App>,
    Json(req): Json<ScheduleFeedIngestRequest>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let files = serde_json::to_value(&req.files).map_err(|e| internal_error(e.into()))?;
    queries::insert_schedule_feed_ingest(&app.database, req.delivered_at, req.ingested_at, &files)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: 1 }))
}

/// `crates/schedule-reference`'s per-sequence batch of resolved
/// STANOX/CRS rows -- see `queries::upsert_stanox_crs`.
async fn post_stanox_crs(
    State(app): State<App>,
    Json(records): Json<Vec<common::StanoxCrsRecord>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_stanox_crs(&app.database, &records)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `trust-consumer`'s periodic live-table reload -- returns the full
/// current table, not a freshness timestamp (see `queries::list_stanox_crs`'s
/// own doc comment for why this route differs from every `last_*_fetch`
/// GET elsewhere in this file).
async fn get_stanox_crs(
    State(app): State<App>,
) -> Result<Json<Vec<common::StanoxCrsRecord>>, (StatusCode, String)> {
    let rows = queries::list_stanox_crs(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

/// `crates/schedule-reference`'s per-line CIF SCHEDULE population publish
/// (POST, its own existing writer credential) and
/// `crates/full-coverage-consumer`'s reload (GET, a new credential) --
/// see `queries::{upsert,get}_schedule_line_population`. Unlike every
/// other GET in this file, this one returns the actual current row for one
/// `(line_id, service_date)`, not a freshness timestamp -- its real reader
/// needs the rows themselves, mirroring `/stanox-crs`'s shape (see
/// docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md's
/// Correction 2).
#[derive(Debug, Deserialize)]
struct SchedulePopulationParams {
    line_id: String,
    service_date: chrono::NaiveDate,
}

#[derive(Debug, Deserialize)]
struct SchedulePopulationBody {
    line_id: String,
    service_date: chrono::NaiveDate,
    population: serde_json::Value,
}

async fn post_schedule_line_population(
    State(app): State<App>,
    Json(body): Json<SchedulePopulationBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    queries::upsert_schedule_line_population(
        &app.database,
        &body.line_id,
        body.service_date,
        &body.population,
    )
    .await
    .map_err(internal_error)?;
    Ok(StatusCode::OK)
}

async fn get_schedule_line_population(
    State(app): State<App>,
    axum::extract::Query(params): axum::extract::Query<SchedulePopulationParams>,
) -> Result<Json<Option<serde_json::Value>>, (StatusCode, String)> {
    let population =
        queries::get_schedule_line_population(&app.database, &params.line_id, params.service_date)
            .await
            .map_err(internal_error)?;
    Ok(Json(population))
}

/// `crates/schedule-reference`'s per-cycle batch of CIF-derived per-station
/// departures -- see `queries::upsert_schedule_network_departures`. POST
/// only: unlike `/schedule-line-population`, no service reads this table
/// back over HTTP -- `api` serves it straight off Postgres via
/// `routes::departures::get_station_schedule_departures`. See
/// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
/// Decision 1.
async fn post_schedule_network_departures(
    State(app): State<App>,
    Json(rows): Json<Vec<ScheduleNetworkDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_schedule_network_departures(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `full-coverage-consumer`'s own periodic snapshot write/read-back --
/// unlike `/schedule-line-population`, both methods here share the SAME
/// group (`internal_oauth_group_full_coverage`), matching `/incidents`'s
/// "one producer, one group, both methods" shape rather than
/// `/stanox-crs`'s split, since this GET is only ever this producer
/// re-checking its own last write, not a second, different caller (see
/// Correction 2).
async fn post_full_coverage_stats(
    State(app): State<App>,
    Json(rows): Json<Vec<common::FullCoverageLineStatsRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_full_coverage_line_stats(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

async fn get_full_coverage_stats_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_full_coverage_line_stats_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "ingestion upsert failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "ingestion failed".to_string(),
    )
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::{Value, json};
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    const FIXTURE_LINE_ID: &str = "ZTEST";

    /// Copied from `routes::station_stats::db_tests::test_app` (that
    /// module's own doc comment: colocated per-file rather than shared,
    /// until a third file needs it too). Every field an inert placeholder
    /// except `database`, which the caller supplies -- these tests touch
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
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
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

    async fn delete_fixture(pool: &PgPool, crs: &str, operator: &str) {
        sqlx::query("DELETE FROM station_full_coverage_samples WHERE crs = $1 AND operator = $2")
            .bind(crs)
            .bind(operator)
            .execute(pool)
            .await
            .expect("cleanup fixture station_full_coverage_samples row");
    }

    /// Distinct name from `delete_fixture` above -- same "reserved
    /// fixture namespace" spirit, applied to `schedule_line_population`'s
    /// own `line_id` key instead of a (crs, operator) pair.
    async fn delete_population_fixture(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_line_population rows");
    }

    fn sample_body(crs: &str, operator: &str, resolved_at: chrono::DateTime<chrono::Utc>) -> Value {
        json!([{
            "crs": crs,
            "operator": operator,
            "resolved_at": resolved_at,
            "stats": {
                "total": 10, "delayed": 2, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 3.5
            }
        }])
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_post_one_row_upserts_and_lands_with_the_right_shape() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFA", "ZA").await;

        let resolved_at = chrono::Utc::now();
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/station-full-coverage-samples")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&sample_body("ZFA", "ZA", resolved_at)).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"upserted": 1}));

        let stats: serde_json::Value = sqlx::query_scalar(
            "SELECT stats FROM station_full_coverage_samples WHERE crs = 'ZFA' AND operator = 'ZA'",
        )
        .fetch_one(&pool)
        .await
        .expect("row landed");
        assert_eq!(
            stats,
            serde_json::json!({
                "total": 10, "delayed": 2, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 3.5
            })
        );

        delete_fixture(&pool, "ZFA", "ZA").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_repeat_post_updates_in_place_not_duplicated() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFB", "ZB").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let first_resolved_at = chrono::Utc::now();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/station-full-coverage-samples")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&sample_body("ZFB", "ZB", first_resolved_at)).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let second_resolved_at = first_resolved_at + chrono::Duration::minutes(1);
        let second_body = json!([{
            "crs": "ZFB",
            "operator": "ZB",
            "resolved_at": second_resolved_at,
            "stats": {
                "total": 20, "delayed": 5, "cancelled": 0, "skipped": 1, "avgDelayMinutes": 1.0
            }
        }]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/station-full-coverage-samples")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&second_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT crs, operator FROM station_full_coverage_samples WHERE crs = 'ZFB' AND operator = 'ZB'",
        )
        .fetch_all(&pool)
        .await
        .expect("query rows");
        assert_eq!(
            rows.len(),
            1,
            "row should be updated in place, not duplicated"
        );

        let stats: serde_json::Value = sqlx::query_scalar(
            "SELECT stats FROM station_full_coverage_samples WHERE crs = 'ZFB' AND operator = 'ZB'",
        )
        .fetch_one(&pool)
        .await
        .expect("row present");
        assert_eq!(
            stats,
            serde_json::json!({
                "total": 20, "delayed": 5, "cancelled": 0, "skipped": 1, "avgDelayMinutes": 1.0
            })
        );

        delete_fixture(&pool, "ZFB", "ZB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_get_last_fetched_after_seeding_is_not_null() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFC", "ZC").await;

        let resolved_at = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats) \
             VALUES ('ZFC', 'ZC', $1, '{\"total\":1,\"delayed\":0,\"cancelled\":0,\"skipped\":0,\"avgDelayMinutes\":0.0}')",
        )
        .bind(resolved_at)
        .execute(&pool)
        .await
        .expect("seed fixture row");

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/station-full-coverage-samples")
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
        let fetched_at = json["fetchedAt"]
            .as_str()
            .expect("fetchedAt should be a non-null timestamp string");
        let fetched_at: chrono::DateTime<chrono::Utc> = fetched_at.parse().unwrap();
        assert!(
            (fetched_at - resolved_at).num_seconds().abs() < 5,
            "fetchedAt {fetched_at} should be close to the seeded resolved_at {resolved_at}"
        );

        delete_fixture(&pool, "ZFC", "ZC").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                station_full_coverage_samples -- --ignored --test-threads=1`"]
    async fn station_full_coverage_samples_get_last_fetched_on_an_empty_table_is_null() {
        // No fixture row is seeded by this test at all, on either the CRS
        // this test uses or otherwise -- `last_station_full_coverage_samples_fetch`
        // is a bare `MAX(resolved_at)` over the whole table (unlike every
        // other query in this module, it isn't scoped by CRS), so this
        // assertion relies on the plan's own binding Non-goal that no real
        // producer writes any row into this table yet (see the plan's
        // Non-goals section) -- in any test/CI database this table is
        // therefore expected to be genuinely empty, not forced empty by a
        // destructive TRUNCATE against a real deployment's table.
        let pool = connect().await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/station-full-coverage-samples")
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
        assert_eq!(json, serde_json::json!({"fetchedAt": null}));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn post_then_get_round_trips_the_exact_population_json() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let population = serde_json::json!([
            {"uid": "C11052", "calling_points": []},
        ]);
        queries::upsert_schedule_line_population(&pool, FIXTURE_LINE_ID, service_date, &population)
            .await
            .expect("seed population");

        let fetched = queries::get_schedule_line_population(&pool, FIXTURE_LINE_ID, service_date)
            .await
            .expect("fetch population");
        assert_eq!(fetched, Some(population));

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn a_second_post_for_the_same_key_wholesale_replaces_not_merges() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let first = serde_json::json!([{"uid": "C11052", "calling_points": []}]);
        let second = serde_json::json!([{"uid": "C99999", "calling_points": []}]);

        queries::upsert_schedule_line_population(&pool, FIXTURE_LINE_ID, service_date, &first)
            .await
            .expect("seed first population");
        queries::upsert_schedule_line_population(&pool, FIXTURE_LINE_ID, service_date, &second)
            .await
            .expect("seed second population");

        let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
            "SELECT population FROM schedule_line_population WHERE line_id = $1 AND service_date = $2",
        )
        .bind(FIXTURE_LINE_ID)
        .bind(service_date)
        .fetch_all(&pool)
        .await
        .expect("select fixture rows");

        assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
        assert_eq!(rows[0].0, second);

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn get_for_a_key_never_posted_is_none_not_an_error() {
        let pool = connect().await;
        delete_population_fixture(&pool, "ZNEVER").await;

        let service_date: chrono::NaiveDate = "2026-09-04".parse().unwrap();
        let fetched = queries::get_schedule_line_population(&pool, "ZNEVER", service_date)
            .await
            .expect("query should succeed even with no row");
        assert_eq!(fetched, None);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_line_population -- --ignored --test-threads=1`"]
    async fn http_post_then_get_round_trip_through_the_router() {
        let pool = connect().await;
        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        let post_body = serde_json::json!({
            "line_id": FIXTURE_LINE_ID,
            "service_date": "2026-09-04",
            "population": [{"uid": "C11052", "calling_points": []}],
        });
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-line-population")
                    .header("content-type", "application/json")
                    .body(Body::from(post_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/schedule-line-population?line_id={FIXTURE_LINE_ID}&service_date=2026-09-04"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{"uid": "C11052", "calling_points": []}])
        );

        delete_population_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    async fn delete_full_coverage_fixture(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM full_coverage_line_stats WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture full_coverage_line_stats row");
    }

    fn fixture_row(line_id: &str, availability: &str) -> common::FullCoverageLineStatsRow {
        common::FullCoverageLineStatsRow {
            line_id: line_id.to_string(),
            service_date: "2026-09-04".parse().unwrap(),
            availability: availability.to_string(),
            stats: common::SampleStats {
                total: 10,
                delayed: 2,
                cancelled: 1,
                skipped: 0,
                avg_delay_minutes: 3.5,
            },
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn post_then_last_fetch_is_non_null_and_recent() {
        let pool = connect().await;
        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;

        let before = queries::last_full_coverage_line_stats_fetch(&pool)
            .await
            .expect("query last fetch");

        queries::upsert_full_coverage_line_stats(&pool, &[fixture_row(FIXTURE_LINE_ID, "pending")])
            .await
            .expect("seed full_coverage_line_stats row");

        let after = queries::last_full_coverage_line_stats_fetch(&pool)
            .await
            .expect("query last fetch")
            .expect("a row now exists");
        if let Some(before) = before {
            assert!(after >= before);
        }
        assert!(chrono::Utc::now() - after < chrono::Duration::minutes(1));

        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn a_second_post_for_the_same_line_updates_the_row_in_place() {
        let pool = connect().await;
        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;

        queries::upsert_full_coverage_line_stats(&pool, &[fixture_row(FIXTURE_LINE_ID, "pending")])
            .await
            .expect("seed first row");
        queries::upsert_full_coverage_line_stats(
            &pool,
            &[fixture_row(FIXTURE_LINE_ID, "available")],
        )
        .await
        .expect("seed second row");

        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT availability FROM full_coverage_line_stats WHERE line_id = $1")
                .bind(FIXTURE_LINE_ID)
                .fetch_all(&pool)
                .await
                .expect("select fixture rows");

        assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
        assert_eq!(rows[0].0, "available");

        delete_full_coverage_fixture(&pool, FIXTURE_LINE_ID).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                full_coverage_line_stats -- --ignored --test-threads=1`"]
    async fn last_fetch_against_an_empty_table_is_null() {
        let pool = connect().await;
        sqlx::query("DELETE FROM full_coverage_line_stats")
            .execute(&pool)
            .await
            .expect("clear the whole table for this test");

        let fetched_at = queries::last_full_coverage_line_stats_fetch(&pool)
            .await
            .expect("query should succeed against an empty table");
        assert_eq!(fetched_at, None);
    }

    async fn delete_network_departures_fixture(pool: &PgPool, crs: &str) {
        sqlx::query("DELETE FROM schedule_network_departures WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_network_departures rows");
    }

    fn network_departures_body(crs: &str, service_date: &str) -> Value {
        json!([{
            "crs": crs,
            "service_date": service_date,
            "departures": [{"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"}],
        }])
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_network_departures -- --ignored --test-threads=1`"]
    async fn post_schedule_network_departures_upserts_the_row() {
        let pool = connect().await;
        delete_network_departures_fixture(&pool, "ZQV").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-network-departures")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        network_departures_body("ZQV", "2026-09-04").to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"upserted": 1}));

        let departures: serde_json::Value = sqlx::query_scalar(
            "SELECT departures FROM schedule_network_departures WHERE crs = 'ZQV' AND service_date = '2026-09-04'",
        )
        .fetch_one(&pool)
        .await
        .expect("row landed");
        assert_eq!(departures[0]["uid"], "C11052");

        delete_network_departures_fixture(&pool, "ZQV").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_network_departures -- --ignored --test-threads=1`"]
    async fn a_second_network_departures_post_for_the_same_key_wholesale_replaces_not_merges() {
        let pool = connect().await;
        delete_network_departures_fixture(&pool, "ZQW").await;

        queries::upsert_schedule_network_departures(
            &pool,
            &[ScheduleNetworkDeparturesRow {
                crs: "ZQW".to_string(),
                service_date: "2026-09-04".parse().unwrap(),
                departures: serde_json::json!([{"uid": "C11052", "scheduled": "08:22:00", "destination_crs": "CRE"}]),
            }],
        )
        .await
        .expect("seed first row");
        queries::upsert_schedule_network_departures(
            &pool,
            &[ScheduleNetworkDeparturesRow {
                crs: "ZQW".to_string(),
                service_date: "2026-09-04".parse().unwrap(),
                departures: serde_json::json!([{"uid": "C99999", "scheduled": "09:00:00", "destination_crs": null}]),
            }],
        )
        .await
        .expect("seed second row");

        let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
            "SELECT departures FROM schedule_network_departures WHERE crs = 'ZQW' AND service_date = '2026-09-04'",
        )
        .fetch_all(&pool)
        .await
        .expect("select fixture rows");
        assert_eq!(rows.len(), 1, "wholesale replace, not a second row");
        assert_eq!(rows[0].0[0]["uid"], "C99999");

        delete_network_departures_fixture(&pool, "ZQW").await;
    }
}
