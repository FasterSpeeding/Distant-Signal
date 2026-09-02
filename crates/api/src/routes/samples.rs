//! Read-only endpoint exposing which stations `poller-ldbws` should
//! sample, computed from the line catalogue loaded into `AppState` at
//! startup plus any custom lines stored in the database. Custom lines can
//! be created/deleted at any time, so they're queried fresh on every
//! request rather than cached like the static catalogue.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use common::LineDefinition;

use crate::app::{App, Router};
use crate::data::custom_lines;
use crate::data::samples::dedup_sample_stations;

pub fn router() -> Router {
    Router::new().route("/sample-stations", axum::routing::get(get_sample_stations))
}

async fn get_sample_stations(
    State(app): State<App>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let custom = custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    let mut lines: Vec<LineDefinition> = app.config.lines.to_vec();
    lines.extend(custom.into_iter().map(LineDefinition::from));
    Ok(Json(dedup_sample_stations(&lines)))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "sample-stations query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}
