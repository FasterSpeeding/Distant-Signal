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
use common::{IncidentMessage, LineStatusReport, StationReference, StationSample, TocReference};
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::data::queries;
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
        .route("/tocs", axum::routing::get(get_tocs_last_fetched).post(post_tocs))
        .route(
            "/station-samples",
            axum::routing::get(get_station_samples_last_fetched).post(post_station_samples),
        )
        .route(
            "/tfl-line-status",
            axum::routing::get(get_tfl_line_status_last_fetched).post(post_tfl_line_status),
        )
        .route("/train-events", axum::routing::post(post_train_events))
        .route("/tracked-trains", axum::routing::get(get_active_tracked_trains))
        .route(
            "/schedule-feed-ingests",
            axum::routing::get(get_schedule_feed_last_fetched).post(post_schedule_feed_ingest),
        )
        .route(
            "/stanox-crs",
            axum::routing::get(get_stanox_crs).post(post_stanox_crs),
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
    Ok(Json(UpsertResponse { upserted: events.len() as u64 }))
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
/// per-poll-cycle batch of reference data -- it's one row per delivery
/// sequence, recorded once the whole delivery has been confirmed stable and
/// complete (see `crates/schedule-ingest`).
#[derive(Debug, Deserialize)]
struct ScheduleFeedIngestRequest {
    sequence: i32,
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
    queries::insert_schedule_feed_ingest(&app.database, req.sequence, req.ingested_at, &files)
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
async fn get_stanox_crs(State(app): State<App>) -> Result<Json<Vec<common::StanoxCrsRecord>>, (StatusCode, String)> {
    let rows = queries::list_stanox_crs(&app.database).await.map_err(internal_error)?;
    Ok(Json(rows))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "ingestion upsert failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "ingestion failed".to_string(),
    )
}
