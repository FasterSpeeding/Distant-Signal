//! `/public/preferences`: which lines/stations are pinned to the home
//! page. Fully session-gated, both read and write -- unlike `/public/lines`,
//! whose *reads* stay unauthenticated (see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals), pinned lines/stations are per-user state with no useful
//! anonymous reading, so every handler here requires a resolved session.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{custom_lines, preferences};

pub fn router() -> Router {
    Router::new()
        .route("/preferences", axum::routing::get(get_preferences))
        .route("/preferences/pinned-lines", axum::routing::put(put_pinned_lines))
        .route("/preferences/pinned-stations", axum::routing::put(put_pinned_stations))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesResponse {
    pinned_lines: Vec<String>,
    pinned_stations: Vec<String>,
}

async fn get_preferences(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<PreferencesResponse>, (StatusCode, String)> {
    let pinned_line_ids = preferences::list_pinned_line_ids(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    let custom = custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    let known_line_ids: HashSet<String> = app
        .config
        .lines
        .iter()
        .map(|l| l.id.clone())
        .chain(custom.into_iter().map(|c| c.id))
        .collect();
    let pinned_lines: Vec<String> = pinned_line_ids
        .into_iter()
        .filter(|id| known_line_ids.contains(id))
        .collect();

    let pinned_station_candidates = preferences::list_pinned_station_crs(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    let pinned_stations = preferences::filter_existing_station_crs(&app.database, &pinned_station_candidates)
        .await
        .map_err(internal_error)?;

    Ok(Json(PreferencesResponse { pinned_lines, pinned_stations }))
}

async fn put_pinned_lines(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(ids): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    preferences::replace_pinned_lines(&app.database, &user.id, &ids)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_pinned_stations(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(crs_codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    if crs_codes.iter().any(|crs| crs.len() != 3) {
        return Err((
            StatusCode::BAD_REQUEST,
            "station codes must be exactly 3 characters".to_string(),
        ));
    }

    preferences::replace_pinned_stations(&app.database, &user.id, &crs_codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "preferences operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "operation failed".to_string())
}
