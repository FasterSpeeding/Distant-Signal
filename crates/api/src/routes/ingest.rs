//! Ingestion endpoints for `private_router()`. Each poller POSTs a `Vec<T>`
//! snapshot once per poll cycle; these handlers just deserialize the body
//! (via axum's `Json<T>` extractor — no hand-rolled body validation) and
//! hand it to the matching upsert query.
//!
//! Each POST route also has a same-path GET counterpart (see `router()`)
//! returning when that table was last successfully populated. Pollers call
//! it once at startup, before their poll loop begins, to skip an
//! immediately-redundant first fetch if the existing data is still fresh —
//! see `common::ingest::time_until_next_poll`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use common::{IncidentMessage, StationReference, StationSample, TocReference};
use serde::Serialize;

use crate::app::{App, Router};
use crate::data::queries;

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
        .route("/tocs", axum::routing::get(get_tocs_last_fetched).post(post_tocs))
        .route(
            "/station-samples",
            axum::routing::get(get_station_samples_last_fetched).post(post_station_samples),
        )
}

#[derive(Debug, Serialize)]
struct UpsertResponse {
    upserted: u64,
}

#[derive(Debug, Serialize)]
struct LastFetchedResponse {
    #[serde(rename = "fetchedAt")]
    fetched_at: Option<DateTime<Utc>>,
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

async fn get_tocs_last_fetched(State(app): State<App>) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_tocs_fetch(&app.database).await.map_err(internal_error)?;
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

async fn post_incidents(
    State(app): State<App>,
    Json(incidents): Json<Vec<IncidentMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_incidents(&app.database, &incidents)
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

async fn post_tocs(
    State(app): State<App>,
    Json(tocs): Json<Vec<TocReference>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tocs(&app.database, &tocs)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "ingestion upsert failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "ingestion failed".to_string(),
    )
}
