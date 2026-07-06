//! Ingestion endpoints for `private_router()`. Each poller (Task 3-5, not
//! yet built) POSTs a `Vec<T>` snapshot once per poll cycle; these handlers
//! just deserialize the body (via axum's `Json<T>` extractor — no
//! hand-rolled body validation) and hand it to the matching upsert query.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use common::{IncidentMessage, StationReference, TocReference};
use serde::Serialize;

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new()
        .route("/incidents", axum::routing::post(post_incidents))
        .route("/stations", axum::routing::post(post_stations))
        .route("/tocs", axum::routing::post(post_tocs))
}

#[derive(Debug, Serialize)]
struct UpsertResponse {
    upserted: u64,
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
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
